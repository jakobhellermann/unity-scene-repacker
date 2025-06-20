#![allow(dead_code)]
use std::io::{Read, Seek};
use std::marker::PhantomData;

use rabex::files::SerializedFile;
use rabex::files::serialzedfile::{ObjectInfo, TypeTreeProvider};
use serde_derive::{Deserialize, Serialize};

pub type PathId = i64;
pub type FileId = i32;

#[derive(Debug, Serialize, Deserialize, Default, Copy, Clone, PartialEq, Eq)]
pub struct PPtr {
    pub m_FileID: FileId,
    pub m_PathID: PathId,
}

impl PPtr {
    pub fn local(path_id: PathId) -> PPtr {
        PPtr {
            m_FileID: 0,
            m_PathID: path_id,
        }
    }
    pub fn is_local(self) -> bool {
        self.m_FileID == 0
    }
    pub fn try_deref(self, serialized: &SerializedFile) -> Option<&ObjectInfo> {
        if self.m_PathID == 0 {
            return None;
        }
        serialized.get_object(self.m_PathID)
    }
    pub fn deref(self, serialized: &SerializedFile) -> &ObjectInfo {
        self.try_deref(serialized).unwrap()
    }
}

#[derive(Deserialize)]
pub struct TypedPPtr<T> {
    pub m_FileID: i32,
    pub m_PathID: i64,
    #[serde(skip)]
    marker: PhantomData<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for TypedPPtr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedPPtr")
            .field("m_FileID", &self.m_FileID)
            .field("m_PathID", &self.m_PathID)
            .finish()
    }
}

impl<T> Copy for TypedPPtr<T> {}
#[allow(clippy::non_canonical_clone_impl)]
impl<T> Clone for TypedPPtr<T> {
    fn clone(&self) -> Self {
        Self {
            m_FileID: self.m_FileID,
            m_PathID: self.m_PathID,
            marker: self.marker,
        }
    }
}

impl<T> TypedPPtr<T> {
    pub fn untyped(self) -> PPtr {
        PPtr {
            m_FileID: self.m_FileID,
            m_PathID: self.m_PathID,
        }
    }
    pub fn try_deref(self, serialized: &SerializedFile) -> Option<&ObjectInfo> {
        self.untyped().try_deref(serialized)
    }
    pub fn deref(self, serialized: &SerializedFile) -> &ObjectInfo {
        self.try_deref(serialized).unwrap()
    }

    pub fn try_deref_read<'de>(
        self,
        serialized: &SerializedFile,
        tpk: impl TypeTreeProvider,
        reader: &mut (impl Read + Seek),
    ) -> Option<T>
    where
        T: serde::Deserialize<'de>,
    {
        let info = self.try_deref(serialized)?;
        Some(serialized.read(info, tpk, reader).unwrap())
    }

    pub fn deref_read<'de>(
        self,
        serialized: &SerializedFile,
        tpk: impl TypeTreeProvider,
        reader: &mut (impl Read + Seek),
    ) -> T
    where
        T: serde::Deserialize<'de>,
    {
        let info = self.try_deref(serialized).unwrap();
        serialized.read(info, tpk, reader).unwrap()
    }
}
