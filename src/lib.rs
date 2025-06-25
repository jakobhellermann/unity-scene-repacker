pub mod env;
mod scene_lookup;
mod trace_pptr;
pub mod typetree_generator_api;
mod unity;

use byteorder::{ByteOrder, LittleEndian};
pub use rabex;

use anyhow::{Context, Result, ensure};
use indexmap::{IndexMap, IndexSet};
use log::warn;
use memmap2::Mmap;
use rabex::files::bundlefile::{
    self, BundleFileBuilder, BundleFileHeader, BundleFileReader, CompressionType, ExtractionConfig,
};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::serializedfile::{Endianness, SerializedType};
use rabex::files::{SerializedFile, serializedfile};
use rabex::objects::pptr::{FileId, PPtr, PathId};
use rabex::objects::{ClassId, ClassIdType};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rabex::{UnityVersion, serde_typetree};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use typetree_generator_api::cache::TypeTreeGeneratorCache;
use typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity::types::MonoBehaviour;

use crate::env::Environment;
use crate::scene_lookup::SceneLookup;
use crate::unity::types::{AssetBundle, AssetInfo, BuildSettings, PreloadData, Transform};

pub struct RepackScene {
    pub scene_name: String,
    pub serialized: SerializedFile,
    pub serialized_path: PathBuf,

    pub keep_objects: BTreeSet<i64>,
    pub scene_paths: Vec<String>,
    pub roots: Vec<i64>,
    pub replacements: FxHashMap<i64, Vec<u8>>,
}

pub fn repack_scenes(
    game_dir: &Path,
    preloads: IndexMap<String, Vec<String>>,
    tpk: &(impl TypeTreeProvider + Send + Sync),
    temp_dir: &Path,
    disable_roots: bool,
) -> Result<Vec<RepackScene>> {
    ensure!(
        game_dir.exists(),
        "Game Directory '{}' does not exist",
        game_dir.display()
    );

    let bundle = game_dir.join("data.unity3d");
    let repack_scenes = if bundle.exists() {
        let mut repack_scenes = Vec::new();
        let mut reader = BundleFileReader::from_reader(
            BufReader::new(File::open(bundle)?),
            &ExtractionConfig::default(),
        )?;

        let mut scenes = None;
        while let Some(mut item) = reader.next() {
            if item.path == "globalgamemanagers" {
                let mut ggm_reader = Cursor::new(item.read()?);
                let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

                let build_settings = ggm
                    .find_object_of::<BuildSettings>(&tpk)?
                    .unwrap()
                    .read(&mut ggm_reader)?;

                scenes = Some(
                    build_settings
                        .scene_names()
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>(),
                );
            } else if let Some(index) = item.path.strip_prefix("level")
                && let Ok(val) = index.parse::<usize>()
            {
                let scenes = scenes
                    .as_ref()
                    .context("globalgamemanagers not found in data.unity3d before level files")?;
                let scene_name = &scenes[val];
                let Some(paths) = preloads.get(scene_name.as_str()) else {
                    continue;
                };

                let data = item.read()?;

                let mut replacements = FxHashMap::default();
                let scene_paths = deduplicate_objects(scene_name, paths);
                let (serialized, keep_objects, roots) = prune_scene(
                    scene_name,
                    Cursor::new(data),
                    tpk,
                    &scene_paths,
                    &mut replacements,
                    disable_roots,
                )?;

                let tmp = temp_dir.join(scene_name);
                std::fs::write(&tmp, data).context("Writing bundle data to temporary file")?;

                repack_scenes.push(RepackScene {
                    scene_name: scene_name.clone(),
                    serialized,
                    serialized_path: tmp,
                    keep_objects,
                    scene_paths: scene_paths.into_iter().map(ToOwned::to_owned).collect(),
                    roots,
                    replacements,
                });
            }
        }
        repack_scenes
    } else {
        let mut ggm_reader = File::open(game_dir.join("globalgamemanagers"))
            .context("couldn't find globalgamemanagers in game directory")?;
        let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

        let scenes = ggm
            .find_object_of::<BuildSettings>(&tpk)?
            .unwrap()
            .read(&mut ggm_reader)?;
        let scenes: FxHashMap<_, _> = scenes
            .scene_names()
            .enumerate()
            .map(|(i, path)| (path, i))
            .collect();

        use rayon::prelude::*;
        preloads
            .into_par_iter()
            .map(|(scene_name, paths)| -> Result<_> {
                let scene_index = scenes[scene_name.as_str()];
                let serialized_path = game_dir.join(format!("level{scene_index}"));

                let file = File::open(&serialized_path)?;
                let data = Cursor::new(unsafe { Mmap::map(&file)? });

                let mut replacements = FxHashMap::default();
                let scene_paths = deduplicate_objects(&scene_name, &paths);
                let (serialized, keep_objects, roots) = prune_scene(
                    &scene_name,
                    data,
                    tpk,
                    &scene_paths,
                    &mut replacements,
                    disable_roots,
                )?;
                Ok(RepackScene {
                    scene_name,
                    serialized,
                    serialized_path,
                    keep_objects,
                    scene_paths: scene_paths.into_iter().map(ToOwned::to_owned).collect(),
                    roots,
                    replacements,
                })
            })
            .collect::<Result<_>>()?
    };
    Ok(repack_scenes)
}

#[inline(never)]
fn prune_scene(
    scene_name: &str,
    mut data: impl Read + Seek,
    tpk: impl TypeTreeProvider,
    retain_paths: &IndexSet<&str>,
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    disable_roots: bool,
) -> Result<(SerializedFile, BTreeSet<PathId>, Vec<PathId>)> {
    let serialized = SerializedFile::from_reader(&mut data)
        .with_context(|| format!("Could not parse {scene_name}"))?;

    let scene_lookup = SceneLookup::new(&serialized, tpk, &mut data)?;
    let retain_objects: Vec<_> = retain_paths
        .into_iter()
        .filter_map(|path| {
            let item = scene_lookup.lookup_path_id(&mut data, path).unwrap();
            if item.is_none() {
                warn!("Could not find path '{path}' in {scene_name}");
            }
            item
        })
        .collect();

    let mut all_reachable = scene_lookup
        .reachable(&retain_objects, &mut data)
        .with_context(|| format!("Could not determine reachable nodes in {scene_name}"))?;

    let mut ancestors = Vec::new();

    for &retain in &retain_objects {
        let mut current = retain;
        loop {
            let transform = serialized
                .get_object::<Transform>(current, &scene_lookup.tpk)?
                .read(&mut data)?;
            let Some(father) = transform.m_Father.optional() else {
                break;
            };
            current = father.m_PathID;

            if !all_reachable.insert(father.m_PathID) {
                break;
            }

            ancestors.push(father.m_PathID);
        }
    }

    for ancestor in ancestors {
        let transform_obj = serialized.get_object::<Transform>(ancestor, &scene_lookup.tpk)?;
        let mut transform = transform_obj.read(&mut data)?;
        transform
            .m_Children
            .retain(|child| all_reachable.contains(&child.m_PathID));

        // TODO disable go? enable but disable components?
        /*let go_obj = transform
            .m_GameObject
            .deref_local(&serialized, &scene_lookup.tpk)?;
        let mut go = go_obj.read(&mut data)?;*/

        all_reachable.insert(transform.m_GameObject.m_PathID);

        let transform_modified = serde_typetree::to_vec_endianed(
            &transform,
            &transform_obj.tt,
            serialized.m_Header.m_Endianess,
        )?;
        assert!(
            replacements
                .insert(transform_obj.info.m_PathID, transform_modified)
                .is_none()
        );
    }

    for settings in serialized
        .objects()
        .filter(|info| [ClassId::RenderSettings].contains(&info.m_ClassID))
    {
        all_reachable.insert(settings.m_PathID);
    }

    for &root in &retain_objects {
        adjust_roots(
            replacements,
            &scene_lookup.tpk,
            &serialized,
            data.by_ref(),
            root,
            disable_roots,
        )?;
    }

    Ok((
        serialized,
        all_reachable,
        retain_objects.into_iter().collect(),
    ))
}

fn adjust_roots(
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    tpk: &impl TypeTreeProvider,
    serialized: &SerializedFile,
    data: &mut (impl Read + Seek),
    transform: i64,
    disable: bool,
) -> Result<(), anyhow::Error> {
    let transform_obj = serialized.get_object::<Transform>(transform, tpk)?;
    let transform = transform_obj.read(data)?;

    if disable {
        let go = transform.m_GameObject.deref_local(serialized, tpk)?;
        let mut go_data = go.read(data)?;
        go_data.m_IsActive = false;
        let go_modified =
            serde_typetree::to_vec_endianed(&go_data, &go.tt, serialized.m_Header.m_Endianess)?;
        assert!(replacements.insert(go.info.m_PathID, go_modified).is_none());
    }

    Ok(())
}

#[must_use]
fn remap_vecs_all<I, T>(old: &mut Vec<T>, new: &mut Vec<T>) -> FxHashMap<I, I>
where
    I: std::hash::Hash + std::cmp::Eq + TryFrom<usize>,
    <I as TryFrom<usize>>::Error: Debug,
{
    std::mem::take(old)
        .into_iter()
        .enumerate()
        .map(|(old_index, ty)| {
            let new_index = new.len();
            new.push(ty);
            (old_index.try_into().unwrap(), new_index.try_into().unwrap())
        })
        .filter(|(old, new)| old != new)
        .collect()
}

#[must_use]
fn remap_vecs<T>(
    used_types: FxHashSet<i32>,
    old: &mut Vec<T>,
    new: &mut Vec<T>,
) -> FxHashMap<i32, i32> {
    let mut old_to_new: FxHashMap<i32, i32> = FxHashMap::default();

    for (old_index, ty) in std::mem::take(old)
        .into_iter()
        .enumerate()
        .filter(|&(idx, _)| used_types.contains(&(idx as i32)))
    {
        let new_index = new.len();
        old_to_new.insert(old_index as i32, new_index as i32);
        new.push(ty);
    }
    old_to_new
}

fn prune_types(serialized: &mut SerializedFile) -> FxHashMap<i32, i32> {
    let used_types: FxHashSet<_> = serialized.objects().map(|obj| obj.m_TypeID).collect();
    let mut old_to_new: FxHashMap<i32, i32> = FxHashMap::default();
    serialized.m_Types = std::mem::take(&mut serialized.m_Types)
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

#[derive(Debug)]
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

            let file = File::open(&scene.serialized_path)?;
            let data = Cursor::new(unsafe { Mmap::map(&file)? });

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.get_ref().len();

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
                        Cow::Borrowed(&data.get_ref()[offset..offset + size])
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

pub fn pack_to_asset_bundles_separate(
    game_dir: &Path,
    out_dir: &Path,
    bundle_name: &str,
    tpk_blob: &TpkTypeTreeBlob,
    tpk: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    scenes: &mut [RepackScene],
) -> Result<()> {
    let common_offset_map = serializedfile::build_common_offset_map(tpk_blob, unity_version);

    let env = Environment::new_in(game_dir, tpk);

    let generator = TypeTreeGenerator::new(unity_version, GeneratorBackend::AssetStudio)?;
    generator
        .load_all_dll_in_dir(game_dir.join("Managed"))
        .context("Cannot load game DLLs")?;
    let generator_cache = TypeTreeGeneratorCache::new(generator);

    for scene in scenes {
        let name = format!("{}_{}", bundle_name, scene.scene_name);

        let serialized = &mut scene.serialized;
        let file = File::open(&scene.serialized_path)?;
        let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

        assert_eq!(serialized.m_bigIDEnabled, None);
        assert!(serialized.m_RefTypes.as_ref().is_some_and(|x| x.is_empty()));

        let mb_types = serialized
            .objects_of::<MonoBehaviour>(tpk)?
            .map(|mb_info| {
                let mb = mb_info.read(&mut data)?;
                let script = env.deref_read(mb.m_Script, serialized, &mut data)?;
                let full_ty =
                    generator_cache.generate(&script.m_AssemblyName, &script.full_name())?;
                Ok((mb_info.info.m_PathID, full_ty))
            })
            .collect::<Result<FxHashMap<_, _>>>()?;

        let mut builder = SerializedFileBuilder::new(unity_version, tpk, &common_offset_map, false);

        serialized.modify_objects(|objects| {
            objects.retain(|obj| scene.keep_objects.contains(&obj.m_PathID))
        });

        let mut path_id_remap = FxHashMap::default();

        builder._next_path_id = 2;
        for obj in serialized.objects() {
            builder.get_next_path_id();
            path_id_remap.insert(obj.m_PathID, builder.get_next_path_id());
        }

        let mut file_id_remap = FxHashMap::default();
        for (i, external) in serialized.m_Externals.iter().enumerate() {
            let orig_file_id = i + 1;
            let new_file_id = builder.serialized.m_Externals.len() + 1;
            file_id_remap.insert(orig_file_id as FileId, new_file_id as FileId);
            builder.serialized.m_Externals.push(external.clone());
        }

        let remap_script_types = remap_vecs_all::<i16, _>(
            serialized.m_ScriptTypes.as_mut().unwrap_or(&mut vec![]),
            builder.serialized.m_ScriptTypes.get_or_insert_default(),
        );
        for ty in serialized.m_ScriptTypes.as_deref_mut().unwrap_or_default() {
            ty.m_LocalSerializedFileIndex = *file_id_remap
                .get(&(ty.m_LocalIdentifierInFile as i32))
                .unwrap_or(&ty.m_LocalSerializedFileIndex);
        }
        for ty in &mut serialized.m_Types {
            ty.m_ScriptTypeIndex = *remap_script_types
                .get(&ty.m_ScriptTypeIndex)
                .unwrap_or(&ty.m_ScriptTypeIndex);
        }

        let container = scene
            .roots
            .iter()
            .zip(scene.scene_paths.iter())
            .map(|(&transform, scene_path)| {
                let transform = serialized
                    .get_object::<Transform>(transform, tpk)?
                    .read(&mut data)?;

                let mut go = transform.m_GameObject;
                assert!(go.is_local());
                if let Some(replacement) = path_id_remap.get(&go.m_PathID) {
                    go.m_PathID = *replacement;
                }

                let info = AssetInfo {
                    asset: go.untyped(),
                    ..Default::default()
                };
                let path = format!("{}.prefab", scene_path.to_lowercase());

                Ok((path, info))
            })
            .collect::<Result<_>>()?;

        let used_types: FxHashSet<_> = serialized.objects().map(|obj| obj.m_TypeID).collect();
        let type_remap = remap_vecs(
            used_types,
            &mut serialized.m_Types,
            &mut builder.serialized.m_Types,
        );
        // serialized.m_types empty from this point

        let new_objects = serialized.take_objects();
        let objects = new_objects.into_iter().map(|mut obj| -> Result<_> {
            obj.m_TypeID = type_remap[&obj.m_TypeID];

            let tt = match mb_types.get(&obj.m_PathID) {
                Some(ty) => ty,
                None => &*builder.serialized.get_typetree_for(&obj, tpk)?,
            };

            let object_data = match scene.replacements.remove(&obj.m_PathID) {
                Some(owned) => Cow::Owned(owned),
                None => {
                    let offset = obj.m_Offset as usize;
                    let size = obj.m_Size as usize;
                    Cow::Borrowed(&data.get_ref()[offset..offset + size])
                }
            };

            let replacement = replace_pptrs_endianed(
                &object_data,
                tt,
                &path_id_remap,
                &file_id_remap,
                serialized.m_Header.m_Endianess,
            )?;

            obj.m_PathID = *path_id_remap.get(&obj.m_PathID).unwrap_or(&obj.m_PathID);

            Ok((obj, Cow::Owned(replacement)))
        });

        // ---

        for object in objects {
            builder.objects.push(object?);
        }

        let assetbundle_ty = tpk
            .get_typetree_node(AssetBundle::CLASS_ID, unity_version)
            .unwrap();
        let mut assetbundle_serialized_type =
            SerializedType::simple(ClassId::AssetBundle, Some(assetbundle_ty.into_owned()));
        assetbundle_serialized_type.m_OldTypeHash = [
            // TODO compute
            151, 218, 95, 70, 136, 228, 90, 87, 200, 180, 45, 79, 66, 73, 114, 151,
        ];
        let ab_type_id = builder.add_type_uncached(assetbundle_serialized_type);
        builder.add_object_inner(
            &AssetBundle {
                m_Name: name.clone(),
                m_PreloadTable: Vec::new(),
                m_Container: container,
                m_MainAsset: AssetInfo::default(),
                m_RuntimeCompatibility: 1,
                m_AssetBundleName: name.clone(),
                m_IsStreamedSceneAssetBundle: false,
                m_PathFlags: 7,
                ..Default::default()
            },
            1,
            ab_type_id,
        )?;
        builder.objects.sort_by_key(|(info, _)| info.m_PathID);

        let mut serialized_out = Vec::new();
        builder.write(&mut Cursor::new(&mut serialized_out))?;

        let bundle_out = File::create(out_dir.join(format!("{}.bundle", &scene.scene_name)))?;
        let mut bundle_builder = BundleFileBuilder::unityfs(7, unity_version);
        bundle_builder.add_file(&format!("CAB-{name}"), Cursor::new(serialized_out))?;
        bundle_builder.write(&mut BufWriter::new(bundle_out), CompressionType::None)?;
    }

    Ok(())
}

#[inline(never)]
fn replace_pptrs_endianed(
    value: &[u8],
    ty: &TypeTreeNode,
    path_id_remap: &FxHashMap<PathId, PathId>,
    file_id_remap: &FxHashMap<FileId, FileId>,
    endianness: Endianness,
) -> Result<Vec<u8>> {
    match endianness {
        serializedfile::Endianness::Little => {
            replace_pptrs::<LittleEndian>(value, ty, path_id_remap, file_id_remap)
        }
        serializedfile::Endianness::Big => {
            replace_pptrs::<LittleEndian>(value, ty, path_id_remap, file_id_remap)
        }
    }
}

fn replace_pptrs<B: ByteOrder + 'static>(
    value: &[u8],
    tt: &TypeTreeNode,
    path_id_remap: &FxHashMap<PathId, PathId>,
    file_id_remap: &FxHashMap<FileId, FileId>,
) -> Result<Vec<u8>> {
    let mut reader = Cursor::new(value);
    let mut de = serde_typetree::Deserializer::<_, B>::from_reader(&mut reader, tt);

    let mut out = Vec::new();
    let mut writer = Cursor::new(&mut out);
    let mut ser = serde_typetree::Serializer::<_, B>::new(&mut writer, tt);

    let mut val = serde_json::Value::deserialize(&mut de)?;
    remap_paths(&mut val, path_id_remap, file_id_remap);
    val.serialize(&mut ser).context("ser")?;

    Ok(out)
}

fn remap_paths(
    val: &mut serde_json::Value,
    path_id_remap: &FxHashMap<PathId, PathId>,
    file_id_remap: &FxHashMap<FileId, FileId>,
) {
    use serde_json::Value::*;

    match val {
        Null | Bool(_) | Number(_) | String(_) => {}

        Array(values) => {
            for val in values {
                remap_paths(val, path_id_remap, file_id_remap);
            }
        }
        Object(map) => {
            if map.keys().len() == 2
                && let Some(file_id) = map.get("m_FileID")
                && let Some(path_id) = map.get("m_PathID")
            {
                let file_id = file_id.as_number().unwrap().as_i64().unwrap();
                let path_id = path_id.as_number().unwrap().as_i64().unwrap();
                if file_id == 0
                    && let Some(&replacement) = path_id_remap.get(&path_id)
                {
                    map["m_PathID"] = replacement.into();
                }

                if let Some(&replacement) = file_id_remap.get(&(file_id as i32)) {
                    map["m_FileID"] = replacement.into();
                }
            } else {
                for val in map.values_mut() {
                    remap_paths(val, path_id_remap, file_id_remap);
                }
            }
        }
    }
}
