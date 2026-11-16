use anyhow::{Context, Result};
use log::warn;
use rabex::files::SerializedFile;
use rabex::files::serializedfile::ObjectInfo;
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::objects::pptr::{FileId, PPtr, PathId};
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rabex_env::trace_pptr::map_pptrs_inplace;
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Debug;
use std::path::PathBuf;

use crate::scene_name_display;

pub struct RemapSerializedIndices {
    pub path_id: FxHashMap<PathId, PathId>,
    pub file_id: FxHashMap<FileId, FileId>,
    pub types: FxHashMap<i32, i32>,
}

/// Takes the metadata (types, externals etc.) from `file` and moves them into `builder`.
pub fn add_scene_meta_to_builder(
    builder: &mut SerializedFileBuilder<impl TypeTreeProvider>,
    file: &mut SerializedFile,
) -> Result<RemapSerializedIndices> {
    if let Some(ref_types) = &file.m_RefTypes
        && !ref_types.is_empty()
    {
        warn!(
            "Found {} m_RefTypes in scene, this is untested.",
            ref_types.len(),
        );
    }

    let mut remap_path_id = FxHashMap::default();
    for obj in file.objects() {
        remap_path_id.insert(obj.m_PathID, builder.get_next_path_id());
    }

    let mut remap_file_id = FxHashMap::default();
    for (i, external) in file.m_Externals.iter().enumerate() {
        let new_file_id = builder.get_or_insert_external_identifier(external);
        remap_file_id.insert(FileId::from_externals_index(i), new_file_id);
    }
    for ty in file.m_ScriptTypes.as_deref_mut().unwrap_or_default() {
        ty.m_LocalSerializedFileIndex = *remap_file_id
            // .get(&(ty.m_LocalIdentifierInFile as i32)) TODO: this was previously here??
            .get(&ty.m_LocalSerializedFileIndex)
            .unwrap_or(&ty.m_LocalSerializedFileIndex);
    }
    // TODO: deduplicate
    let remap_script_types = remap_vecs_all::<i16, _>(
        file.m_ScriptTypes.as_mut().unwrap_or(&mut vec![]),
        builder.serialized.m_ScriptTypes.get_or_insert_default(),
    );
    for ty in &mut file.m_Types {
        ty.m_ScriptTypeIndex = *remap_script_types
            .get(&ty.m_ScriptTypeIndex)
            .unwrap_or(&ty.m_ScriptTypeIndex);
    }
    let used_types: FxHashSet<_> = file.objects().map(|obj| obj.m_TypeID).collect();
    let remap_types = remap_vecs(
        used_types,
        &mut file.m_Types,
        &mut builder.serialized.m_Types,
    );

    Ok(RemapSerializedIndices {
        path_id: remap_path_id,
        file_id: remap_file_id,
        types: remap_types,
    })
}

pub struct RemappedObject {
    pub info: ObjectInfo,
    pub data: Cow<'static, [u8]>,
    pub references: Vec<PathId>,
    pub externals: Vec<FileId>,
}

pub fn remap_objects(
    scene_name: &str,
    original_name: PathBuf,
    file: &SerializedFile,
    data: &[u8],
    tpk: &impl TypeTreeProvider,
    objects: Vec<ObjectInfo>,
    mut replacements: FxHashMap<PathId, Vec<u8>>,
    mb_types: FxHashMap<PathId, &TypeTreeNode>,
    remap: RemapSerializedIndices,
) -> impl Iterator<Item = Result<RemappedObject>> {
    objects.into_iter().map(move |mut obj| -> Result<_> {
        obj.m_TypeID = remap.types[&obj.m_TypeID];

        let tt = match mb_types.get(&obj.m_PathID) {
            Some(ty) => ty,
            // TODO: take types from file if they exist
            None => &*file.get_typetree_for(&obj, &tpk)?,
        };
        let mut object_data = match replacements.remove(&obj.m_PathID) {
            Some(owned) => Cow::Owned(owned),
            None => {
                let offset = obj.m_Offset as usize;
                let size = obj.m_Size as usize;
                Cow::Borrowed(&data[offset..offset + size])
            }
        };

        let orig_path_id = obj.m_PathID;
        obj.m_PathID = *remap.path_id.get(&obj.m_PathID).unwrap_or(&obj.m_PathID);

        let mut references = Vec::new();
        let mut externals = Vec::new();
        map_pptrs_inplace(
            object_data.to_mut().as_mut_slice(),
            tt,
            file.m_Header.m_Endianess,
            |node, pptr| {
                if pptr.is_null() {
                    return Ok(pptr);
                }
                if pptr.is_local() {
                    let Some(&path_id) = remap.path_id.get(&pptr.m_PathID) else {
                        warn!(
                            "{}: object {orig_path_id} references {} outside the repacked objects, setting it to null",
                            scene_name_display(scene_name, &original_name),
                            pptr.m_PathID
                        );
                        return Ok(PPtr::null());
                    };
                    if node.m_Name != "m_Father" {
                        references.push(path_id);
                    }
                    return Ok(PPtr::local(path_id));
                }
                let file_id = *remap.file_id.get(&pptr.m_FileID).unwrap_or(&pptr.m_FileID);
                externals.push(file_id);
                Ok(PPtr::new(file_id, pptr.m_PathID))
            },
        )
        .with_context(|| {
            format!(
                "Could not remap path IDs in bundle for {orig_path_id} in {}:\n{}",
                scene_name_display(scene_name, &original_name),
                tt.dump_pretty()
            )
        })?;

        Ok(RemappedObject {
            info: obj,
            data: Cow::Owned(object_data.into_owned()),
            references,
            externals,
        })
    })
}

/// Moves the elements from `old` into `new`, returning where each index ended up in
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
