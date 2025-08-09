pub mod env;
mod game_files;
mod merge_serialized;
pub mod monobehaviour_typetree_export;
mod prune;
mod reachable;
pub mod scene_lookup;
mod trace_pptr;
pub mod typetree_generator_cache;
pub mod unity;

pub use game_files::GameFiles;
pub use {rabex, typetree_generator_api};

use anyhow::{Context, Result};
use indexmap::{IndexMap, IndexSet};
use log::warn;
use rabex::UnityVersion;
use rabex::files::bundlefile::{BundleFileBuilder, CompressionType};
use rabex::files::serializedfile::FileIdentifier;
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::{SerializedFile, serializedfile};
use rabex::objects::pptr::{PPtr, PathId};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::io::{Cursor, Read, Seek, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use unity::types::MonoBehaviour;

use crate::env::{EnvResolver, Environment};
use crate::game_files::Data;
use crate::scene_lookup::SceneLookup;
use crate::unity::types::{
    AssetBundle, AssetInfo, BuildSettings, MonoScript, PreloadData, Transform,
};

pub struct RepackSettings {
    pub scene_objects: IndexMap<String, Vec<String>>,
    pub extra_objects: IndexMap<String, IndexSet<String>>,
}
impl RepackSettings {
    pub fn is_empty(&self) -> bool {
        self.scene_objects.is_empty() && self.extra_objects.is_empty()
    }
}

pub struct RepackScene<'a> {
    pub original_name: String,
    pub scene_name: Option<String>,

    pub serialized: SerializedFile,
    pub serialized_data: Data,

    pub keep_objects: BTreeSet<i64>,
    pub roots: Vec<(String, Transform)>,
    pub replacements: FxHashMap<PathId, Vec<u8>>,

    pub monobehaviour_types: FxHashMap<i64, &'a TypeTreeNode>,
}

// Filename, PathId, Classname, Objectname
type ExtraObject = (String, PathId, String, String);

pub fn repack_scenes<'a>(
    env: &'a Environment,
    repack_settings: RepackSettings,
    prepare_scripts: bool,
    disable_roots: bool,
) -> Result<(Vec<RepackScene<'a>>, Vec<ExtraObject>)> {
    let (scenes, extra_objects) = collect_what_to_repack(
        env,
        &repack_settings,
        |filename, scene_name, object_paths, file, data| {
            let settings = RepackSceneSettings {
                object_paths,
                disable_roots,
            };
            repack_scene(
                env,
                prepare_scripts,
                filename,
                Some(scene_name),
                settings,
                file,
                data,
            )
        },
    )?;

    for (class_name, names) in repack_settings.extra_objects {
        for name in names {
            let found = extra_objects
                .iter()
                .any(|(_, _, found_class_name, found_name)| {
                    class_name == *found_class_name && name == *found_name
                });
            if !found {
                warn!("Did not found {class_name} named '{name}' in game files");
            }
        }
    }

    Ok((scenes, extra_objects))
}

// TODO: this is a mess, refactor it away
fn collect_what_to_repack<T: Send + Sync>(
    env: &Environment,
    repack_settings: &RepackSettings,
    f: impl Fn(&str, &str, &[String], SerializedFile, Data) -> Result<T> + Send + Sync,
) -> Result<(Vec<T>, Vec<ExtraObject>)> {
    let (ggm, mut ggm_reader) = env.load_cached("globalgamemanagers")?;
    let build_settings = ggm
        .find_object_of::<BuildSettings>(&env.tpk)?
        .unwrap()
        .read(&mut ggm_reader)?;

    let has_extra_objects = !repack_settings.extra_objects.is_empty();

    let read = |filename: &str, scene_name| -> Result<_> {
        let data = env.resolver.read(filename).with_context(|| {
            format!(
                "{} not exist in bundle",
                scene_name_display(scene_name, filename)
            )
        })?;

        let mut reader = Cursor::new(data.as_ref());
        let file = SerializedFile::from_reader(&mut reader).with_context(|| {
            format!(
                "Could not parse {}",
                scene_name_display(scene_name, filename)
            )
        })?;

        Ok((data, file))
    };

    // if we have extra monobehaviour to collect, we have to look at every possible file
    let (extra_objects, scenes) = if has_extra_objects {
        let scene_lookup: Vec<_> = build_settings.scene_names().collect();

        env.resolver
            .serialized_files()?
            .into_par_iter()
            .map(|filename| -> Result<_> {
                let filename = filename.to_str().expect("non-utf8 scene path");

                let (data, file) = read(filename, None)?;

                let mut reader = Cursor::new(data.as_ref());
                let extra_objects = find_extra_objects(
                    env,
                    &file,
                    filename,
                    &mut reader,
                    &repack_settings.extra_objects,
                )?;

                if let Some(scene_index) = filename
                    .strip_prefix("level")
                    .and_then(|x| x.parse::<usize>().ok())
                {
                    let scene_name = scene_lookup[scene_index];

                    if let Some(object_paths) = repack_settings.scene_objects.get(scene_name) {
                        let x = f(filename, scene_name, object_paths, file, data)?;
                        return Ok((extra_objects, Some(x)));
                    }
                }

                Ok((extra_objects, None))
            })
            .try_fold(
                || (Vec::new(), Vec::new()),
                |mut acc, item| -> Result<_> {
                    let item = item?;
                    acc.0.extend(item.0);
                    if let Some(scene) = item.1 {
                        acc.1.push(scene);
                    }
                    Ok(acc)
                },
            )
            .try_reduce(
                || (Vec::new(), Vec::new()),
                |mut acc, item| {
                    acc.0.extend(item.0);
                    acc.1.extend(item.1);
                    Ok(acc)
                },
            )?
    } else {
        let scene_lookup = build_settings.scene_name_lookup();

        let scenes = repack_settings
            .scene_objects
            .par_iter()
            .map(|(scene_name, object_paths)| -> Result<_> {
                let scene_index = *scene_lookup
                    .get(scene_name)
                    .with_context(|| format!("Scene '{scene_name}' was not found in game files"))?;
                let filename = format!("level{scene_index}");

                let (data, file) = read(&filename, None)?;
                f(&filename, scene_name, object_paths, file, data)
            })
            .collect::<Result<Vec<_>>>()?;
        (Vec::new(), scenes)
    };

    Ok((scenes, extra_objects))
}

struct RepackSceneSettings<'a> {
    object_paths: &'a [String],
    disable_roots: bool,
}

fn repack_scene<'a>(
    env: &'a Environment,
    prepare_scripts: bool,
    original_name: &str,
    scene_name: Option<&str>,
    settings: RepackSceneSettings,
    file: SerializedFile,
    serialized_data: Data,
) -> Result<RepackScene<'a>> {
    let reader = &mut Cursor::new(serialized_data.as_ref());

    let scene_paths = deduplicate_objects(original_name, scene_name, settings.object_paths);

    let mut replacements = FxHashMap::default();
    let (keep_objects, roots) = prune::prune_scene(
        env,
        scene_name,
        original_name,
        &file,
        reader,
        &scene_paths,
        &mut replacements,
        settings.disable_roots,
    )?;

    let monobehaviour_types = prepare_scripts
        .then(|| prepare_monobehaviour_types(env, &file, reader))
        .transpose()
        .with_context(|| {
            format!(
                "Could not generate type trees in {}",
                scene_name_display(scene_name, original_name)
            )
        })?
        .unwrap_or_default();

    Ok(RepackScene {
        original_name: original_name.to_owned(),
        scene_name: scene_name.map(ToOwned::to_owned),
        serialized: file,
        serialized_data,
        keep_objects,
        roots,
        replacements,
        monobehaviour_types,
    })
}

fn find_extra_objects(
    env: &Environment,
    file: &SerializedFile,
    filename: &str,
    reader: &mut (impl Read + Seek),
    // classname: [objectname]
    extra_objects: &IndexMap<String, IndexSet<String>>,
) -> Result<Vec<(String, PathId, String, String)>, anyhow::Error> {
    let mut roots = Vec::new();

    for mb_obj in file.objects_of::<MonoBehaviour>(&env.tpk)? {
        let Some(script) = file.script_type(mb_obj.info) else {
            continue;
        };
        let script = env.deref_read(script.typed::<MonoScript>(), file, reader)?;

        let Some(mb_names) = extra_objects.get(&script.m_ClassName) else {
            continue;
        };

        let mb = mb_obj.read(reader)?;
        if mb_names.contains(&mb.m_Name) {
            roots.push((
                filename.to_owned(),
                mb_obj.info.m_PathID,
                script.m_ClassName,
                mb.m_Name,
            ));
        }
    }
    Ok(roots)
}

fn scene_name_display(scene_name: Option<&str>, original_name: &str) -> String {
    match scene_name {
        Some(scene_name) => format!("{scene_name} ({original_name}'"),
        None => format!("'{original_name}'"),
    }
}

fn prune_types(file: &mut SerializedFile) -> FxHashMap<i32, i32> {
    let used_types: FxHashSet<_> = file.objects().map(|obj| obj.m_TypeID).collect();
    let mut old_to_new: FxHashMap<i32, i32> = FxHashMap::default();
    file.m_Types = std::mem::take(&mut file.m_Types)
        .into_iter()
        .enumerate()
        .filter(|&(idx, _)| used_types.contains(&(idx as i32)))
        .enumerate()
        .map(|(new_index, (old_index, ty))| {
            old_to_new.insert(old_index as i32, new_index as i32);
            ty
        })
        .collect();
    old_to_new
}

fn deduplicate_objects<'a>(
    original_name: &str,
    scene_name: Option<&str>,
    paths: &'a [String],
) -> IndexSet<&'a str> {
    let mut deduplicated = IndexSet::new();
    for item in paths {
        if !deduplicated.insert(item.as_str()) {
            warn!(
                "Duplicate object: '{item}' in {}",
                scene_name_display(scene_name, original_name)
            );
        }
    }
    deduplicated
}

#[derive(Debug, Default)]
pub struct Stats {
    pub objects_before: usize,
    pub objects_after: usize,
    pub size_before: usize,
    pub size_after: usize,
}

pub fn pack_to_scene_bundle(
    writer: impl Write + Seek,
    bundle_name: &str,
    tpk_blob: &TpkTypeTreeBlob,
    tpk: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    scenes: &mut [RepackScene],
    compression: CompressionType,
) -> Result<Stats> {
    let mut stats = Stats::default();

    let common_offset_map = serializedfile::build_common_offset_map(tpk_blob, unity_version);

    let container = scenes
        .iter()
        .filter_map(|scene| scene.scene_name.as_deref())
        .map(|scene_name| {
            let path = get_scene_bundle_scene_name(bundle_name, scene_name);
            (path, AssetInfo::default())
        })
        .collect();
    let mut container = Some(container);

    let mut builder = BundleFileBuilder::unityfs(7, unity_version);

    for scene in scenes {
        let scene_name = scene
            .scene_name
            .as_deref()
            .expect("non-scene file found for scene bundle");

        let mut sharedassets =
            SerializedFileBuilder::new(unity_version, tpk, &common_offset_map, true);
        sharedassets.add_object(&PreloadData {
            m_Name: "".into(),
            m_Assets: vec![PPtr {
                m_FileID: 1,
                m_PathID: 10001,
            }],
            ..Default::default()
        })?;
        if let Some(container) = container.take() {
            sharedassets.add_object(&AssetBundle {
                m_Name: bundle_name.to_owned(),
                m_Container: container,
                m_MainAsset: AssetInfo::default(),
                m_RuntimeCompatibility: 1,
                m_IsStreamedSceneAssetBundle: true,
                m_PathFlags: 7,
                ..Default::default()
            })?;
        }

        let mut out = Cursor::new(Vec::new());
        sharedassets.write(&mut out)?;

        builder.add_file(
            &format!("BuildPlayer-{bundle_name}_{scene_name}.sharedAssets",),
            Cursor::new(out.into_inner()),
        )?;

        let trimmed = {
            let serialized = &mut scene.serialized;

            let data = scene.serialized_data.as_ref();

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.len();

            serialized.modify_objects(|objects| {
                objects.retain(|obj| scene.keep_objects.contains(&obj.m_PathID));
            });
            stats.objects_after += serialized.objects().len();

            let type_remap = prune_types(serialized);

            let new_objects = serialized.take_objects();
            let objects = new_objects.into_iter().map(|mut obj| {
                let data = match scene.replacements.remove(&obj.m_PathID) {
                    Some(owned) => Cow::Owned(owned),
                    None => {
                        let offset = obj.m_Offset as usize;
                        let size = obj.m_Size as usize;
                        Cow::Borrowed(&data[offset..offset + size])
                    }
                };

                obj.m_TypeID = type_remap[&obj.m_TypeID];

                (obj, data)
            });

            let mut writer = Cursor::new(Vec::new());
            serializedfile::write_serialized_with_objects(
                &mut writer,
                serialized,
                &common_offset_map,
                objects,
            )?;
            let out = writer.into_inner();

            stats.objects_after += serialized.objects().len();
            stats.size_after += out.len();

            out
        };
        builder.add_file(
            &format!("BuildPlayer-{bundle_name}_{scene_name}"),
            Cursor::new(trimmed),
        )?;
    }

    builder.write(writer, compression)?;

    Ok(stats)
}

pub enum MonobehaviourTypetreeMode<'a> {
    GenerateRuntime,
    Export(&'a [u8]),
}

pub fn pack_to_asset_bundle(
    env: &Environment,
    writer: impl Write + Seek,
    bundle_name: &str,
    tpk_blob: &TpkTypeTreeBlob,
    scenes: Vec<RepackScene>,
    extra_objects: Vec<ExtraObject>,
    compression: CompressionType,
) -> Result<Stats> {
    let unity_version = env.unity_version()?;
    let common_offset_map = serializedfile::build_common_offset_map(tpk_blob, unity_version);

    let mut builder =
        SerializedFileBuilder::new(unity_version, &env.tpk, &common_offset_map, false);
    builder._next_path_id = 2;

    let mut container = IndexMap::new();

    for (filename, path_id, class_name, object_name) in extra_objects {
        // TODO cached
        let file_id = builder.add_external_uncached(FileIdentifier::new(filename));
        let info = AssetInfo::new(PPtr::new(file_id, path_id));
        container.insert(get_extra_object_asset_name(&class_name, &object_name), info);
    }

    let mut stats = Stats::default();

    let intermediate = scenes
        .into_iter()
        .map(|mut scene| {
            let serialized = &mut scene.serialized;
            let data = scene.serialized_data.as_ref();

            assert_eq!(serialized.m_bigIDEnabled, None);
            assert!(serialized.m_RefTypes.as_ref().is_some_and(|x| x.is_empty()));

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.len();
            serialized.modify_objects(|objects| {
                objects.retain(|obj| scene.keep_objects.contains(&obj.m_PathID))
            });
            stats.objects_after += serialized.objects().len();

            let remap = merge_serialized::add_scene_meta_to_builder(&mut builder, serialized)?;

            for (scene_path, transform) in scene.roots.iter() {
                let mut go = transform.m_GameObject;
                assert!(go.is_local());
                if let Some(replacement) = remap.path_id.get(&go.m_PathID) {
                    go.m_PathID = *replacement;
                }

                let scene_name = scene
                    .scene_name
                    .as_deref()
                    .expect("right now every asset comes from a scene");
                let info = AssetInfo::new(go.untyped());
                let path = get_asset_bundle_object_asset_name(scene_name, scene_path);

                container.insert(path, info);
            }

            Ok((scene, remap))
        })
        .collect::<Result<Vec<_>>>()?;

    let objects = intermediate
        .into_par_iter()
        .map(|(mut scene, remap)| {
            merge_serialized::remap_objects(
                scene.scene_name.as_deref(),
                scene.original_name,
                &builder.serialized,
                scene.serialized_data.as_ref(),
                &env.tpk,
                scene.serialized.take_objects(),
                scene.replacements,
                scene.monobehaviour_types,
                remap,
            )
            .collect::<Vec<_>>()
        })
        .collect::<Vec<Vec<Result<_>>>>();

    objects.into_iter().try_for_each(|objects| -> Result<_> {
        for obj in objects {
            builder.objects.push(obj?);
        }
        Ok(())
    })?;

    builder._next_path_id = 1;
    builder.add_object(&AssetBundle {
        m_Name: bundle_name.to_owned(),
        m_PreloadTable: Vec::new(),
        m_Container: container,
        m_MainAsset: AssetInfo::default(),
        m_RuntimeCompatibility: 1,
        m_AssetBundleName: bundle_name.to_owned(),
        m_IsStreamedSceneAssetBundle: false,
        m_PathFlags: 7,
        ..Default::default()
    })?;

    builder.objects.sort_by_key(|(info, _)| info.m_PathID);

    let mut out = Vec::new();
    builder.write(&mut Cursor::new(&mut out))?;
    stats.size_after += out.len();

    let mut bundle_builder = BundleFileBuilder::unityfs(7, unity_version);
    bundle_builder.add_file(&format!("CAB-{bundle_name}"), Cursor::new(out))?;

    bundle_builder.write(writer, compression)?;

    Ok(stats)
}

#[inline(never)]
fn prepare_monobehaviour_types<'a>(
    env: &'a Environment,
    file: &SerializedFile,
    reader: &mut (impl Read + Seek),
) -> Result<FxHashMap<i64, &'a TypeTreeNode>> {
    Ok(file
        .objects_of::<MonoBehaviour>(&env.tpk)?
        .map(|mb_info| -> Result<_> {
            let path_id = mb_info.info.m_PathID;

            let mb = mb_info.read(reader)?;
            if mb.m_Script.is_null() {
                return Ok(None);
            }
            let script = env
                .deref_read(mb.m_Script, file, reader)
                .with_context(|| format!("In monobehaviour {}", mb_info.info.m_PathID))?;

            let (assembly_name, full_name) = script.into_location();
            let assembly_name = match assembly_name.ends_with(".dll") {
                true => assembly_name,
                false => format!("{assembly_name}.dll"),
            };

            let ty = env
                .typetree_generator
                .generate(&assembly_name, &full_name)
                .with_context(|| {
                    format!("Reading script {assembly_name} {full_name} at object {path_id}",)
                })?;

            Ok(Some((path_id, ty)))
        })
        .filter_map(|x| x.transpose())
        .filter_map(|ty| match ty {
            Ok(val) => Some(val),
            Err(e) => {
                log::error!("{e:?}");
                None
            }
        })
        .collect::<FxHashMap<_, _>>())
}

pub fn pack_to_shallow_asset_bundle(
    env: &Environment,
    writer: impl Write + Seek,
    bundle_name: &str,
    repack_settings: RepackSettings,
    compression: CompressionType,
) -> Result<Stats> {
    let objects_before = AtomicUsize::new(0);
    let size_before = AtomicUsize::new(0);

    let (scene_objects, extra_objects) = collect_what_to_repack(
        env,
        &repack_settings,
        |filename, scene_name, object_paths, file, data| {
            let object_paths = deduplicate_objects(filename, Some(scene_name), object_paths);

            objects_before.fetch_add(file.objects().len(), Ordering::Relaxed);
            size_before.fetch_add(data.as_ref().len(), Ordering::Relaxed);

            let reader = &mut Cursor::new(data.as_ref());

            let mut path_ids = Vec::with_capacity(object_paths.len());
            let lookup = SceneLookup::new(&file, reader, &env.tpk)?;
            for path in object_paths {
                let Some((_, transform)) = lookup.lookup_path(reader, path)? else {
                    warn!("Could not find path '{path}' in {scene_name}");
                    continue;
                };
                path_ids.push((path.to_owned(), transform.m_GameObject.m_PathID));
            }

            Ok(((scene_name.to_owned(), filename.to_owned()), path_ids))
        },
    )?;

    create_shallow_assetbundle(
        env,
        writer,
        bundle_name,
        scene_objects,
        extra_objects,
        compression,
    )?;

    Ok(Stats {
        objects_before: objects_before.into_inner(),
        objects_after: 0,
        size_before: size_before.into_inner(),
        size_after: 0,
    })
}

fn create_shallow_assetbundle(
    env: &Environment,
    writer: impl Write + Seek,
    bundle_name: &str,
    // (scenename, Filename)
    objects: Vec<((String, String), Vec<(String, PathId)>)>,
    extra_objects: Vec<ExtraObject>,
    compression: CompressionType,
) -> Result<()> {
    let unity_version = env.unity_version()?;
    let common_offset_map = serializedfile::build_common_offset_map(&env.tpk.inner, unity_version);

    let mut builder =
        SerializedFileBuilder::new(unity_version, &env.tpk, &common_offset_map, false);

    let mut container = IndexMap::default();

    for (filename, path_id, class_name, object_name) in extra_objects {
        // TODO cached
        let file_id = builder.add_external_uncached(FileIdentifier::new(filename));
        let info = AssetInfo::new(PPtr::new(file_id, path_id));
        container.insert(get_extra_object_asset_name(&class_name, &object_name), info);
    }

    for ((scene_name, scene_index), objects) in objects {
        let file_index =
            builder.add_external_uncached(FileIdentifier::new(format!("level{scene_index}")));

        for (scene_path, path_id) in objects {
            let path = get_asset_bundle_object_asset_name(&scene_name, &scene_path);
            let info = AssetInfo::new(PPtr::new(file_index, path_id));
            container.insert(path, info);
        }
    }

    let ab = AssetBundle {
        m_Name: bundle_name.to_owned(),
        m_Container: container,
        m_MainAsset: AssetInfo::default(),
        m_RuntimeCompatibility: 1,
        m_IsStreamedSceneAssetBundle: false,
        m_PathFlags: 7,
        ..Default::default()
    };
    builder._next_path_id = 1;
    builder.add_object(&ab)?;

    let mut builder_out = Vec::new();
    builder.write(Cursor::new(&mut builder_out))?;

    let mut bundle = BundleFileBuilder::unityfs(7, unity_version);
    bundle.add_file(&format!("CAB-{bundle_name}"), Cursor::new(builder_out))?;

    bundle.write(writer, compression)?;

    Ok(())
}

fn get_scene_bundle_scene_name(bundle_name: &str, scene_name: &str) -> String {
    format!("unity-scene-repacker/{bundle_name}_{scene_name}.unity")
}
fn get_asset_bundle_object_asset_name(scene_name: &str, scene_path: &str) -> String {
    format!("{scene_name}/{scene_path}.prefab").to_lowercase()
}
fn get_extra_object_asset_name(class_name: &str, object_name: &str) -> String {
    format!("ExtraObjects/{class_name}/{object_name}.prefab").to_lowercase()
}
