pub use rabex;

use anyhow::{Context, Result};
use indexmap::IndexSet;
use log::warn;
use rabex::files::SerializedFile;
use rabex::objects::pptr::PathId;
use rabex::objects::{ClassId, ClassIdType};
use rabex::serde_typetree;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rustc_hash::FxHashMap;
use std::collections::{BTreeSet, VecDeque};
use std::io::{Read, Seek};

use crate::scene_lookup::SceneLookup;
use crate::unity::types::Transform;

pub fn prune_scene(
    scene_name: &str,
    file: &SerializedFile,
    reader: &mut (impl Read + Seek),
    tpk: &impl TypeTreeProvider,
    retain_paths: &IndexSet<&str>,
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    disable_roots: bool,
) -> Result<(BTreeSet<PathId>, Vec<(String, Transform)>)> {
    let scene_lookup = SceneLookup::new(file, &mut *reader, tpk)?;

    let mut retain_ids = VecDeque::with_capacity(retain_paths.len());
    let mut retain_objects = Vec::with_capacity(retain_paths.len());
    for &path in retain_paths {
        match scene_lookup.lookup_path(&mut *reader, path)? {
            Some((path_id, transform)) => {
                retain_ids.push_back(path_id);
                retain_objects.push((path.to_owned(), transform));
            }
            None => {
                warn!("Could not find path '{path}' in {scene_name}")
            }
        }
    }

    let mut all_reachable = scene_lookup
        .reachable(retain_ids, reader)
        .with_context(|| format!("Could not determine reachable nodes in {scene_name}"))?;

    let mut ancestors = Vec::new();
    for (_, transform) in &retain_objects {
        for ancestor in transform.ancestors(file, reader, tpk)? {
            let (id, transform) = ancestor?;
            if !all_reachable.insert(id) {
                break;
            }

            ancestors.push((id, transform));
        }
    }

    let transform_typetree = file.get_typetree_for_class(Transform::CLASS_ID, tpk)?;

    for (id, transform) in ancestors {
        adjust_ancestor(
            replacements,
            file,
            &mut all_reachable,
            &transform_typetree,
            id,
            transform,
        )?;
    }

    for settings in file
        .objects()
        .filter(|info| [ClassId::RenderSettings].contains(&info.m_ClassID))
    {
        all_reachable.insert(settings.m_PathID);
    }

    for (_, root_transform) in &retain_objects {
        adjust_kept(
            replacements,
            file,
            reader.by_ref(),
            tpk,
            root_transform,
            disable_roots,
        )?;
    }

    Ok((all_reachable, retain_objects))
}

fn adjust_ancestor(
    replacements: &mut FxHashMap<PathId, Vec<u8>>,
    file: &SerializedFile,
    all_reachable: &mut BTreeSet<i64>,
    transform_typetree: &TypeTreeNode,
    id: i64,
    mut transform: Transform,
) -> Result<()> {
    transform
        .m_Children
        .retain(|child| all_reachable.contains(&child.m_PathID));
    all_reachable.insert(transform.m_GameObject.m_PathID);
    let transform_modified =
        serde_typetree::to_vec_endianed(&transform, transform_typetree, file.m_Header.m_Endianess)?;
    assert!(replacements.insert(id, transform_modified).is_none());
    Ok(())
}

fn adjust_kept(
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    file: &SerializedFile,
    data: &mut (impl Read + Seek),
    tpk: &impl TypeTreeProvider,
    transform: &Transform,
    disable: bool,
) -> Result<(), anyhow::Error> {
    if disable {
        let go = transform.m_GameObject.deref_local(file, tpk)?;
        let mut go_data = go.read(data)?;
        go_data.m_IsActive = false;
        let go_modified =
            serde_typetree::to_vec_endianed(&go_data, &go.tt, file.m_Header.m_Endianess)?;
        assert!(replacements.insert(go.info.m_PathID, go_modified).is_none());
    }

    Ok(())
}
