use anyhow::Result;
use rabex::files::SerializedFile;

use rabex::objects::pptr::{PPtr, PathId};
use rabex::typetree::TypeTreeProvider;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{Read, Seek};

use crate::trace_pptr::trace_pptrs;
use crate::unity::types::Transform;

pub struct SceneLookup<'a, P> {
    roots: HashMap<String, (PathId, Transform)>,
    serialized: &'a SerializedFile,
    tpk: P,
}

impl<'a, P: TypeTreeProvider> SceneLookup<'a, P> {
    pub fn new(
        serialized: &'a SerializedFile,
        tpk: P,
        reader: &mut (impl Read + Seek),
    ) -> Result<Self> {
        let mut roots = HashMap::new();

        for transform_obj in serialized.objects_of::<Transform>(&tpk)? {
            let transform = transform_obj.read(reader)?;
            if transform.m_Father.optional().is_some() {
                continue;
            }

            let go = transform
                .m_GameObject
                .deref_local(serialized, &tpk)?
                .read(reader)?;

            roots
                .entry(go.m_Name)
                .or_insert((transform_obj.info.m_PathID, transform));
        }

        Ok(SceneLookup {
            roots,
            serialized,
            tpk,
        })
    }

    pub fn lookup_path_id(
        &self,
        reader: &mut (impl Read + Seek),
        path: &str,
    ) -> Result<Option<PathId>> {
        Ok(self.lookup_path_full(reader, path)?.map(|(id, _)| id))
    }
    pub fn lookup_path_full(
        &self,
        reader: &mut (impl Read + Seek),
        path: &str,
    ) -> Result<Option<(i64, Transform)>> {
        let mut segments = path.split('/');
        let Some(root_name) = segments.next() else {
            return Ok(None);
        };
        let Some(root) = self.roots.get(root_name) else {
            return Ok(None);
        };
        let mut current = vec![root.clone()];

        for segment in segments {
            let mut found = Vec::new();
            for current in &current {
                for child_pptr in &current.1.m_Children {
                    let child = child_pptr
                        .deref_local(self.serialized, &self.tpk)?
                        .read(reader)?;
                    let go = child
                        .m_GameObject
                        .deref_local(self.serialized, &self.tpk)?
                        .read(reader)?;

                    if go.m_Name == segment {
                        found.push((child_pptr.m_PathID, child));
                    }
                }
            }

            current = found;
            if current.is_empty() {
                return Ok(None);
            }
        }

        Ok(current.pop())
    }

    pub fn reachable(
        &self,
        from: &[PathId],
        reader: &mut (impl Read + Seek),
    ) -> Result<BTreeSet<PathId>> {
        let mut queue: VecDeque<PathId> = VecDeque::new();
        queue.extend(from);

        let mut include = BTreeSet::new();

        while let Some(node) = queue.pop_front() {
            include.insert(node);

            let reachable = self.reachable_one(node, reader)?;
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

    fn reachable_one(&self, from: PathId, reader: &mut (impl Read + Seek)) -> Result<Vec<PPtr>> {
        let info = self.serialized.get_object_info(from).unwrap();

        let tt = self
            .tpk
            .get_typetree_node(info.m_ClassID, self.serialized.m_UnityVersion.unwrap())
            .unwrap();
        reader.seek(std::io::SeekFrom::Start(info.m_Offset as u64))?;
        trace_pptrs(&tt, reader)
    }
}
