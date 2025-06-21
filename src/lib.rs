pub mod env;
mod scene_lookup;
mod trace_pptr;
pub mod typetree_generator_api;
mod unity;

pub use rabex;

use anyhow::{Context, Result, ensure};
use indexmap::{IndexMap, IndexSet};
use log::warn;
use memmap2::Mmap;
use rabex::files::bundlefile::{self, BundleFileHeader, BundleFileReader, ExtractionConfig};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::{SerializedFile, serializedfile};
use rabex::objects::ClassId;
use rabex::objects::pptr::{PPtr, PathId};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::{UnityVersion, serde_typetree};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use crate::scene_lookup::SceneLookup;
use crate::unity::types::{AssetBundle, AssetInfo, BuildSettings, PreloadData, Transform};

pub struct RepackScene {
    pub scene_name: String,
    pub serialized: SerializedFile,
    pub serialized_path: PathBuf,

    pub keep_objects: BTreeSet<i64>,
    pub roots: Vec<i64>,
    pub replacements: FxHashMap<i64, Vec<u8>>,
}

pub fn repack_scenes(
    game_dir: &Path,
    preloads: IndexMap<String, Vec<String>>,
    tpk: &(impl TypeTreeProvider + Send + Sync),
    temp_dir: &Path,
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
                let (serialized, keep_objects, roots) = prune_scene(
                    scene_name,
                    Cursor::new(data),
                    tpk,
                    deduplicate_objects(scene_name, paths),
                    &mut replacements,
                )?;

                let tmp = temp_dir.join(scene_name);
                std::fs::write(&tmp, data).context("Writing bundle data to temporary file")?;

                repack_scenes.push(RepackScene {
                    scene_name: scene_name.clone(),
                    serialized,
                    serialized_path: tmp,
                    keep_objects,
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
                let (serialized, keep_objects, roots) = prune_scene(
                    &scene_name,
                    data,
                    tpk,
                    deduplicate_objects(&scene_name, &paths),
                    &mut replacements,
                )?;
                Ok(RepackScene {
                    scene_name,
                    serialized,
                    serialized_path,
                    keep_objects,
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
    retain_paths: IndexSet<&str>,
    replacements: &mut FxHashMap<i64, Vec<u8>>,
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

    Ok((
        serialized,
        all_reachable,
        retain_objects.into_iter().collect(),
    ))
}

fn adjust_roots(
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    tpk: &impl TypeTreeProvider,
    serialized: &mut SerializedFile,
    data: &mut Cursor<Mmap>,
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

pub fn repack_bundle(
    bundle_name: &str,
    tpk_blob: &TpkTypeTreeBlob,
    tpk: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    disable_roots: bool,
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
        let mut sharedassets = SerializedFileBuilder::new(unity_version, tpk, &common_offset_map);

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
            let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.get_ref().len();

            serialized.modify_objects(|objects| {
                objects.retain(|obj| scene.keep_objects.contains(&obj.m_PathID));
            });
            stats.objects_after += serialized.objects().len();

            let type_remap = prune_types(serialized);

            for &root in scene.roots.iter() {
                adjust_roots(
                    &mut scene.replacements,
                    tpk,
                    serialized,
                    &mut data,
                    root,
                    disable_roots,
                )?;
            }

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
