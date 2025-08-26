use std::io::Cursor;
use std::path::Path;

use anyhow::{Context as _, Result};
use rabex::files::SerializedFile;
use rabex::files::serializedfile::ObjectRef;
use rabex::objects::pptr::PathId;
use rabex::objects::{ClassId, ClassIdType, TypedPPtr};
use rabex::typetree::TypeTreeProvider;
use serde::Deserialize;

use crate::env::{EnvResolver, Environment};
use crate::unity::types::MonoScript;

pub struct SerializedFileHandle<'a, R, P> {
    pub file: &'a SerializedFile,
    pub data: &'a [u8],
    pub env: &'a Environment<R, P>,
}
pub struct ObjectRefHandle<'a, T, R, P> {
    pub object: ObjectRef<'a, T>,
    pub file: SerializedFileHandle<'a, R, P>,
}

impl<'a, R: EnvResolver, P: TypeTreeProvider> SerializedFileHandle<'a, R, P> {
    fn reborrow(&self) -> SerializedFileHandle<'a, R, P> {
        SerializedFileHandle {
            file: self.file,
            data: self.data,
            env: self.env,
        }
    }

    pub fn new(env: &'a Environment<R, P>, file: &'a SerializedFile, data: &'a [u8]) -> Self {
        SerializedFileHandle { file, data, env }
    }

    pub fn reader(&self) -> Cursor<&'a [u8]> {
        Cursor::new(self.data)
    }

    pub fn find_object_of<T: ClassIdType + for<'de> Deserialize<'de>>(&self) -> Result<Option<T>> {
        let Some(data) = self.file.find_object_of::<T>(&self.env.tpk)? else {
            return Ok(None);
        };
        Ok(Some(data.read(&mut self.reader())?))
    }

    pub fn objects_of<T>(&self) -> Result<impl Iterator<Item = ObjectRefHandle<'a, T, R, P>>>
    where
        T: ClassIdType,
    {
        let iter = self.file.objects_of::<T>(&self.env.tpk)?;
        Ok(iter.map(|o| ObjectRefHandle::new(o, self.reborrow())))
    }

    pub fn deref<T: for<'de> Deserialize<'de>>(
        &self,
        pptr: TypedPPtr<T>,
    ) -> Result<ObjectRefHandle<'a, T, R, P>> {
        Ok(match pptr.m_FileID {
            0 => {
                let object = pptr.deref_local(self.file, &self.env.tpk)?;
                ObjectRefHandle::new(object, self.reborrow())
            }
            file_id => {
                let external_info = &self.file.m_Externals[file_id as usize - 1];
                let external = self
                    .env
                    .load_external_file(Path::new(&external_info.pathName))?;
                let object = pptr
                    .make_local()
                    .deref_local(external.file, &self.env.tpk)
                    .with_context(|| {
                        format!("In external {} {}", file_id, external_info.pathName)
                    })?;
                ObjectRefHandle::new(object, external)
            }
        })
    }

    pub fn deref_read<T: for<'de> Deserialize<'de>>(&self, pptr: TypedPPtr<T>) -> Result<T> {
        self.deref(pptr)?.read()
    }
}

impl<'a, T, R: EnvResolver, P: TypeTreeProvider> ObjectRefHandle<'a, T, R, P> {
    pub fn new(object: ObjectRef<'a, T>, file: SerializedFileHandle<'a, R, P>) -> Self {
        ObjectRefHandle { object, file }
    }

    pub fn read(&self) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        if self.object.info.m_ClassID == ClassId::MonoBehaviour {
            if self.object.tt.m_Type == "MonoBehaviour" {
                let with_tt = self.load_typetree()?;
                return with_tt.read();
            }
        }

        let data = self.object.read(&mut self.file.reader())?;
        Ok(data)
    }

    pub fn path_id(&self) -> PathId {
        self.object.info.m_PathID
    }
}

impl<'a, T, R: EnvResolver, P: TypeTreeProvider> ObjectRefHandle<'a, T, R, P> {
    pub fn cast<U>(&'a self) -> ObjectRefHandle<'a, U, R, P> {
        ObjectRefHandle {
            object: self.object.cast(),
            file: self.file.reborrow(),
        }
    }

    fn load_typetree(&'a self) -> Result<ObjectRefHandle<'a, T, R, P>>
    where
        for<'de> T: Deserialize<'de>,
    {
        let script = self
            .mono_script()?
            .with_context(|| format!("MonoBehaviour {} has no MonoScript", self.path_id()))?;
        self.load_typetree_as(&script)
    }

    fn load_typetree_as<U>(&'a self, script: &MonoScript) -> Result<ObjectRefHandle<'a, U, R, P>>
    where
        U: for<'de> Deserialize<'de>,
    {
        let data = self
            .file
            .env
            .load_typetree_as(&self.object.cast(), script)?;

        Ok(ObjectRefHandle {
            object: data,
            file: self.file.reborrow(),
        })
    }

    pub fn mono_script(&self) -> Result<Option<MonoScript>> {
        let Some(script_type) = self.file.file.script_type(self.object.info) else {
            return Ok(None);
        };

        self.file.env.deref_read(
            script_type.typed(),
            &self.file.file,
            &mut self.file.reader(),
        )
    }
}
