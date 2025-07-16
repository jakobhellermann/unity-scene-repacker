use anyhow::Result;
use rabex::files::SerializedFile;
use rabex::objects::PPtr;
use rabex::objects::pptr::PathId;
use rabex::typetree::TypeTreeProvider;
use std::collections::{BTreeSet, VecDeque};
use std::io::{Read, Seek};

use crate::env::Environment;
use crate::trace_pptr;

/// Returns all reachable local objects from the starting point,
/// only going down the transform hierarchy.
pub fn reachable(
    env: &Environment,
    file: &SerializedFile,
    reader: &mut (impl Read + Seek),
    from: VecDeque<PathId>,
) -> Result<BTreeSet<PathId>> {
    let mut queue = from;

    let mut include = BTreeSet::new();

    while let Some(node) = queue.pop_front() {
        include.insert(node);

        let reachable = reachable_one(env, file, node, reader)?;
        for reachable in reachable {
            if !reachable.is_local() {
                continue;
            }

            if include.insert(reachable.m_PathID) {
                queue.push_back(reachable.m_PathID);
            }
        }
    }

    Ok(include)
}

pub fn reachable_one(
    env: &Environment,
    file: &SerializedFile,
    from: PathId,
    reader: &mut (impl Read + Seek),
) -> Result<Vec<PPtr>> {
    let info = file.get_object_info(from).unwrap();
    let tt = env
        .tpk
        .get_typetree_node(info.m_ClassID, file.m_UnityVersion.unwrap())
        .unwrap();
    reader.seek(std::io::SeekFrom::Start(info.m_Offset as u64))?;
    trace_pptr::trace_pptrs_endianned(&tt, reader, file.m_Header.m_Endianess)
}
