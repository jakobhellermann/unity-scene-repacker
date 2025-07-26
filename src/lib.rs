pub mod env;
mod game_files;
mod merge_serialized;
pub mod monobehaviour_typetree_export;
mod prune;
pub mod scene_lookup;
mod trace_pptr;
pub mod typetree_generator_cache;
pub mod unity;

pub use game_files::GameFiles;
pub use {rabex, typetree_generator_api};

use anyhow::{Context, Result};
use indexmap::{IndexMap, IndexSet};
use log::warn;
use memmap2::Mmap;
use rabex::UnityVersion;
use rabex::files::bundlefile::{self, BundleFileBuilder, BundleFileHeader, CompressionType};
use rabex::files::serializedfile::FileIdentifier;
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::{SerializedFile, serializedfile};
use rabex::objects::pptr::{PPtr, PathId};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use unity::types::MonoBehaviour;

use crate::env::Environment;
use crate::game_files::LevelFiles;
use crate::scene_lookup::SceneLookup;
use crate::unity::types::{AssetBundle, AssetInfo, BuildSettings, PreloadData, Transform};

pub struct RepackSettings {
    pub scene_objects: IndexMap<String, Vec<String>>,
}
impl RepackSettings {
    pub fn is_empty(&self) -> bool {
        self.scene_objects.is_empty()
    }
}

pub struct RepackScene<'a> {
    pub scene_name: String,
    pub scene_index: usize,
    pub serialized: SerializedFile,
    pub serialized_data: Data,

    pub keep_objects: BTreeSet<i64>,
    pub roots: Vec<(String, Transform)>,
    pub replacements: FxHashMap<i64, Vec<u8>>,

    pub monobehaviour_types: FxHashMap<i64, &'a TypeTreeNode>,
}

pub fn repack_scenes<'a>(
    env: &'a Environment,
    repack_settings: RepackSettings,
    prepare_scripts: bool,
    disable_roots: bool,
) -> Result<Vec<RepackScene<'a>>> {
    match &env.resolver.level_files {
        LevelFiles::Unpacked => {
            let mut ggm_reader = File::open(env.resolver.game_dir.join("globalgamemanagers"))
                .context("couldn't find globalgamemanagers in game directory")?;
            let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

            let build_settings = ggm
                .find_object_of::<BuildSettings>(&env.tpk)?
                .unwrap()
                .read(&mut ggm_reader)?;
            let scenes = build_settings.scene_name_lookup();

            use rayon::prelude::*;
            repack_settings
                .scene_objects
                .into_par_iter()
                .map(|(scene_name, object_paths)| -> Result<_> {
                    let scene_index = scenes[scene_name.as_str()];
                    let serialized_path = env.resolver.game_dir.join(format!("level{scene_index}"));

                    let file = File::open(&serialized_path)?;
                    let mmap = unsafe { Mmap::map(&file)? };

                    let settings = RepackSceneSettings {
                        object_paths,
                        disable_roots,
                    };
                    repack_scene(
                        env,
                        prepare_scripts,
                        scene_name,
                        scene_index,
                        settings,
                        Data::Mmap(mmap),
                    )
                })
                .collect::<Result<_>>()
        }
        LevelFiles::Packed(bundle) => {
            let ggm = bundle
                .read_at("globalgamemanagers")?
                .context("globalgamemanagers not found in bundle")?;
            let ggm_reader = &mut Cursor::new(ggm);
            let ggm = SerializedFile::from_reader(ggm_reader)?;
            let build_settings = ggm
                .find_object_of::<BuildSettings>(&env.tpk)?
                .unwrap()
                .read(ggm_reader)?;
            let scenes = build_settings.scene_name_lookup();

            use rayon::prelude::*;
            repack_settings
                .scene_objects
                .into_par_iter()
                .map(|(scene_name, object_paths)| -> Result<_> {
                    let scene_index = scenes[scene_name.as_str()];

                    let data = bundle
                        .read_at(&format!("level{scene_index}"))?
                        .with_context(|| {
                            format!("level{scene_index} ({scene_name}) not exist in bundle")
                        })?;
                    let settings = RepackSceneSettings {
                        object_paths,
                        disable_roots,
                    };
                    repack_scene(
                        env,
                        prepare_scripts,
                        scene_name,
                        scene_index,
                        settings,
                        Data::InMemory(data),
                    )
                })
                .collect::<Result<Vec<RepackScene>>>()
        }
    }
}

struct RepackSceneSettings {
    object_paths: Vec<String>,
    disable_roots: bool,
}

fn repack_scene<'a>(
    env: &'a Environment,
    prepare_scripts: bool,
    scene_name: String,
    scene_index: usize,
    settings: RepackSceneSettings,
    serialized_data: Data,
) -> Result<RepackScene<'a>> {
    let reader = &mut Cursor::new(serialized_data.as_ref());

    let scene_paths = deduplicate_objects(&scene_name, &settings.object_paths);

    let file = SerializedFile::from_reader(reader)
        .with_context(|| format!("Could not parse {scene_name}"))?;

    let mut replacements = FxHashMap::default();
    let (keep_objects, roots) = prune::prune_scene(
        &scene_name,
        &file,
        reader,
        &env.tpk,
        &scene_paths,
        &mut replacements,
        settings.disable_roots,
    )?;

    let monobehaviour_types = prepare_scripts
        .then(|| prepare_monobehaviour_types(env, &file, reader))
        .transpose()
        .with_context(|| {
            format!("Could not generate type trees in {scene_name} (level{scene_index})")
        })?
        .unwrap_or_default();

    Ok(RepackScene {
        scene_name,
        scene_index,
        serialized: file,
        serialized_data,
        keep_objects,
        roots,
        replacements,
        monobehaviour_types,
    })
}

pub enum Data {
    InMemory(Vec<u8>),
    Mmap(Mmap),
}
impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        match self {
            Data::InMemory(data) => data.as_slice(),
            Data::Mmap(mmap) => mmap.as_ref(),
        }
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

fn deduplicate_objects<'a>(scene_name: &str, paths: &'a [String]) -> IndexSet<&'a str> {
    let mut deduplicated = IndexSet::new();
    for item in paths {
        if !deduplicated.insert(item.as_str()) {
            warn!("Duplicate object: '{item}' in '{scene_name}'");
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
    bundle_name: &str,
    tpk_blob: &TpkTypeTreeBlob,
    tpk: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    scenes: &mut [RepackScene],
) -> Result<(Stats, BundleFileHeader, Vec<(String, Vec<u8>)>)> {
    let mut files = Vec::new();

    let mut stats = Stats {
        objects_before: 0,
        objects_after: 0,
        size_before: 0,
        size_after: 0,
    };

    let common_offset_map = serializedfile::build_common_offset_map(tpk_blob, unity_version);

    let prefix = bundle_name;
    let container = scenes
        .iter()
        .map(|scene| {
            let path = format!("unity-scene-repacker/{prefix}_{}.unity", scene.scene_name);
            (path, AssetInfo::default())
        })
        .collect();
    let mut container = Some(container);

    for scene in scenes {
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
        out.set_position(0);

        files.push((
            format!("BuildPlayer-{prefix}_{}.sharedAssets", scene.scene_name),
            out.into_inner(),
        ));

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
        files.push((
            format!("BuildPlayer-{prefix}_{}", scene.scene_name),
            trimmed,
        ));
    }

    let header = BundleFileHeader {
        signature: bundlefile::BundleSignature::UnityFS,
        version: 7,
        unity_version: "5.x.x".to_owned(),
        unity_revision: Some(unity_version),
        size: 0, // unused
    };

    Ok((stats, header, files))
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
    unity_version: UnityVersion,
    scenes: Vec<RepackScene>,
    compression: CompressionType,
) -> Result<Stats> {
    let common_offset_map = serializedfile::build_common_offset_map(tpk_blob, unity_version);

    let mut builder =
        SerializedFileBuilder::new(unity_version, &env.tpk, &common_offset_map, false);
    builder._next_path_id = 2;

    let mut container = IndexMap::new();

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

            let mut remap_path_id = FxHashMap::default();

            for obj in serialized.objects() {
                builder.get_next_path_id();
                remap_path_id.insert(obj.m_PathID, builder.get_next_path_id());
            }

            for (scene_path, transform) in scene.roots.iter() {
                let mut go = transform.m_GameObject;
                assert!(go.is_local());
                if let Some(replacement) = remap_path_id.get(&go.m_PathID) {
                    go.m_PathID = *replacement;
                }

                let path = format!("{}/{}.prefab", scene.scene_name, scene_path).to_lowercase();
                let info = AssetInfo::new(go.untyped());

                container.insert(path, info);
            }

            let (remap_file_id, remap_types) =
                merge_serialized::add_remapped_scene_header(&mut builder, serialized)?;

            Ok((scene, remap_file_id, remap_path_id, remap_types))
        })
        .collect::<Result<Vec<_>>>()?;

    let objects = intermediate
        .into_par_iter()
        .map(|(mut scene, remap_file_id, remap_path_id, remap_types)| {
            merge_serialized::add_remapped_scene(
                &scene.scene_name,
                scene.scene_index,
                &builder.serialized,
                scene.serialized_data.as_ref(),
                &env.tpk,
                scene.serialized.take_objects(),
                scene.replacements,
                scene.monobehaviour_types,
                remap_file_id,
                remap_path_id,
                remap_types,
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
    let (ggm, mut ggm_reader) = env.load_cached("globalgamemanagers")?;

    let build_settings = ggm
        .find_object_of::<BuildSettings>(&env.tpk)?
        .unwrap()
        .read(&mut ggm_reader)?;
    let scenes = build_settings.scene_name_lookup();

    let objects_before = AtomicUsize::new(0);
    let size_before = AtomicUsize::new(0);

    let all = repack_settings
        .scene_objects
        .into_par_iter()
        .map(|(scene_name, object_paths)| {
            let object_paths = deduplicate_objects(&scene_name, &object_paths);

            let scene_index = *scenes
                .get(&scene_name)
                .with_context(|| format!("Scene '{scene_name}' not found in game files"))?;

            let (file, mut reader) = env.load_leaf(format!("level{scene_index}"))?;
            let reader = &mut reader;

            objects_before.fetch_add(file.objects().len(), Ordering::Relaxed);
            size_before.fetch_add(reader.get_ref().len(), Ordering::Relaxed);

            let mut path_ids = Vec::with_capacity(object_paths.len());
            let lookup = SceneLookup::new(&file, reader, &env.tpk)?;
            for path in object_paths {
                let Some((_, transform)) = lookup.lookup_path(reader, &path)? else {
                    warn!("Could not find path '{path}' in {scene_name}");
                    continue;
                };
                path_ids.push((path.to_owned(), transform.m_GameObject.m_PathID));
            }

            Ok(((scene_name, scene_index), path_ids))
        })
        .collect::<Result<Vec<_>>>()?;

    create_shallow_assetbundle(env, writer, bundle_name, all, compression)?;

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
    objects: Vec<((String, usize), Vec<(String, PathId)>)>,
    compression: CompressionType,
) -> Result<()> {
    let unity_version = env.unity_version()?;
    let common_offset_map = serializedfile::build_common_offset_map(&env.tpk.inner, unity_version);

    let mut builder =
        SerializedFileBuilder::new(unity_version, &env.tpk, &common_offset_map, false);

    let mut container = IndexMap::default();

    for ((scene_name, scene_index), objects) in objects {
        let file_index =
            builder.add_external_uncached(FileIdentifier::new(format!("level{scene_index}")));

        for (scene_path, path_id) in objects {
            let path = format!("{scene_name}/{scene_path}.prefab").to_lowercase();
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
