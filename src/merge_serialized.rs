use anyhow::{Context, Result};
use rabex::files::SerializedFile;
use rabex::files::serializedfile::ObjectInfo;
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::objects::pptr::{FileId, PathId};
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::fmt::Debug;

use crate::trace_pptr::replace_pptrs_inplace_endianed;

pub fn add_remapped_scene(
    scene_name: &str,
    scene_index: usize,
    file: &SerializedFile,
    data: &[u8],
    tpk: &impl TypeTreeProvider,
    objects: Vec<ObjectInfo>,
    mut replacements: FxHashMap<i64, Vec<u8>>,
    mb_types: FxHashMap<i64, &TypeTreeNode>,
    remap_file_id: FxHashMap<FileId, FileId>,
    path_id_remap: FxHashMap<PathId, PathId>,
    remap_types: FxHashMap<i32, i32>,
) -> impl Iterator<Item = Result<(ObjectInfo, Cow<'static, [u8]>)>> {
    objects.into_iter().map(move |mut obj| -> Result<_> {
        obj.m_TypeID = remap_types[&obj.m_TypeID];

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
        obj.m_PathID = *path_id_remap.get(&obj.m_PathID).unwrap_or(&obj.m_PathID);

        replace_pptrs_inplace_endianed(
            object_data.to_mut().as_mut_slice(),
            tt,
            &path_id_remap,
            &remap_file_id,
            file.m_Header.m_Endianess,
        )
        .with_context(|| {
            format!(
                "Could not remap path IDs in bundle for {} in '{}' (level{}):\n{}",
                orig_path_id,
                scene_name,
                scene_index,
                tt.dump_pretty()
            )
        })?;

        Ok((obj, Cow::Owned(object_data.into_owned())))
    })
}

pub fn add_remapped_scene_header(
    builder: &mut SerializedFileBuilder<impl TypeTreeProvider>,
    file: &mut SerializedFile,
) -> Result<(FxHashMap<FileId, FileId>, FxHashMap<i32, i32>)> {
    let mut remap_file_id = FxHashMap::default();
    for (i, external) in file.m_Externals.iter().enumerate() {
        let orig_file_id = i + 1;
        let new_file_id = builder.serialized.m_Externals.len() + 1;
        remap_file_id.insert(orig_file_id as FileId, new_file_id as FileId);
        builder.serialized.m_Externals.push(external.clone());
    }
    let remap_script_types = remap_vecs_all::<i16, _>(
        file.m_ScriptTypes.as_mut().unwrap_or(&mut vec![]),
        builder.serialized.m_ScriptTypes.get_or_insert_default(),
    );
    for ty in file.m_ScriptTypes.as_deref_mut().unwrap_or_default() {
        ty.m_LocalSerializedFileIndex = *remap_file_id
            .get(&(ty.m_LocalIdentifierInFile as i32))
            .unwrap_or(&ty.m_LocalSerializedFileIndex);
    }
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

    Ok((remap_file_id, remap_types))
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
