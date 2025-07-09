use std::borrow::Cow;
use std::io::{Read, Seek};

use rabex::files::serializedfile::{ObjectRef, Result};
use rabex::files::{SerializedFile, serializedfile};
use rabex::objects::ClassIdType;
use rabex::objects::pptr::PathId;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};

use crate::unity::types::{GameObject, Transform};

impl GameObject {
    pub fn transform<'a>(
        &'a self,
        file: &'a SerializedFile,
        tpk: &'a impl TypeTreeProvider,
    ) -> Result<Option<ObjectRef<'a, Transform>>> {
        self.component::<Transform>(file, tpk)
    }

    pub fn component<'a, T: ClassIdType>(
        &'a self,
        file: &'a SerializedFile,
        tpk: &'a impl TypeTreeProvider,
    ) -> Result<Option<ObjectRef<'a, T>>> {
        for component in &self.m_Component {
            let component = component.component.deref_local(file, tpk)?;
            if component.info.m_ClassID == T::CLASS_ID {
                return Ok(Some(component));
            }
        }

        Ok(None)
    }

    pub fn components<'a>(
        &'a self,
        file: &'a SerializedFile,
        tpk: &'a impl TypeTreeProvider,
    ) -> impl Iterator<Item = Result<ObjectRef<'a, ()>>> {
        self.m_Component
            .iter()
            .map(|component| component.component.deref_local(file, tpk))
    }

    pub fn path(
        &self,
        file: &SerializedFile,
        reader: &mut (impl Read + Seek),
        tpk: &impl TypeTreeProvider,
    ) -> Result<String> {
        let mut path = Vec::new();
        path.push(self.m_Name.clone());

        let transform = self.transform(file, tpk)?.unwrap().read(reader)?;

        for ancestor in transform.ancestors(file, reader, tpk)?.collect::<Vec<_>>() {
            let (_, ancestor) = ancestor?;
            let ancestor_go = ancestor.m_GameObject.deref_local(file, tpk)?.read(reader)?;
            path.push(ancestor_go.m_Name);
        }

        path.reverse();
        Ok(path.join("/"))
    }
}

pub struct Ancestors<'a, R> {
    file: &'a SerializedFile,
    reader: &'a mut R,
    next: Option<PathId>,
    transform_typetree: Cow<'a, TypeTreeNode>,
}

impl<R: Read + Seek> Iterator for Ancestors<'_, R> {
    type Item = Result<(PathId, Transform), serializedfile::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let current_id = self.next?;
        let current = (|| {
            let info = self
                .file
                .get_object_info(current_id)
                .ok_or(serializedfile::Error::NoObject(current_id))?;
            self.reader
                .seek(std::io::SeekFrom::Start(info.m_Offset as u64))?;
            let father = rabex::serde_typetree::from_reader_endianed::<Transform>(
                &mut self.reader,
                &self.transform_typetree,
                self.file.m_Header.m_Endianess,
            )
            .map_err(serializedfile::Error::Deserialize)?;

            Ok(father)
        })();
        let current = match current {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };

        self.next = current.m_Father.optional().map(|father| father.m_PathID);

        Some(Ok((current_id, current)))
    }
}

impl Transform {
    pub fn ancestors<'a, R>(
        self: &Transform,
        file: &'a SerializedFile,
        reader: &'a mut R,
        tpk: &'a impl TypeTreeProvider,
    ) -> Result<Ancestors<'a, R>, serializedfile::Error> {
        let transform_typetree = tpk
            .get_typetree_node(Transform::CLASS_ID, file.m_UnityVersion.unwrap())
            .ok_or(serializedfile::Error::NoTypetree(Transform::CLASS_ID))?;
        Ok(Ancestors {
            file,
            reader,
            next: self.m_Father.optional().map(|father| father.m_PathID),
            transform_typetree,
        })
    }
}
