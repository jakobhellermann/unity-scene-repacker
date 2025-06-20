use anyhow::Result;
use rabex::files::SerializedFile;
use rabex::files::serialzedfile::TypeTreeProvider;
use rabex::objects::ClassId;
use rabex::objects::pptr::{PPtr, PathId};
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
    pub fn new(serialized: &'a SerializedFile, tpk: P, reader: &mut (impl Read + Seek)) -> Self {
        let mut roots = HashMap::new();
        for (name, (path_id, transform)) in serialized
            .objects_of_class_id(ClassId::Transform)
            .filter_map(|info| {
                let transform: Transform = serialized.read(info, &tpk, reader).unwrap();
                let None = transform.m_Father.try_deref(serialized) else {
                    return None;
                };
                let go = transform
                    .m_GameObject
                    .deref_read_local(serialized, &tpk, reader)
                    .unwrap();
                Some((go.m_Name, (info.m_PathID, transform)))
            })
        {
            roots.entry(name).or_insert((path_id, transform));
        }

        SceneLookup {
            roots,
            serialized,
            tpk,
        }
    }

    pub fn lookup_path_id(&self, reader: &mut (impl Read + Seek), path: &str) -> Option<PathId> {
        self.lookup_path_full(reader, path).map(|(id, _)| id)
    }
    pub fn lookup_path_full(
        &self,
        reader: &mut (impl Read + Seek),
        path: &str,
    ) -> Option<(i64, Transform)> {
        let mut segments = path.split('/');
        let root_name = segments.next()?;
        let mut current = vec![self.roots.get(root_name)?.clone()];

        for segment in segments {
            let mut found = Vec::new();
            for current in &current {
                for child_pptr in &current.1.m_Children {
                    let child = child_pptr.try_deref_read(self.serialized, &self.tpk, reader)?;
                    let go = child
                        .m_GameObject
                        .deref_read_local(self.serialized, &self.tpk, reader)
                        .unwrap();

                    if go.m_Name == segment {
                        found.push((child_pptr.m_PathID, child));
                    }
                }
            }

            current = found;
            if current.is_empty() {
                return None;
            }
        }

        current.pop()
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
        let info = PPtr::local(from).deref_local(self.serialized);

        let tt = self
            .tpk
            .get_typetree_node(info.m_ClassID, self.serialized.m_UnityVersion.unwrap())
            .unwrap();
        reader.seek(std::io::SeekFrom::Start(info.m_Offset as u64))?;
        trace_pptrs(&tt, reader)
    }
}
