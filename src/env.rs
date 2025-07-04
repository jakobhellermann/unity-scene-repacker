use std::io::{Cursor, Read, Seek};
use std::ops::Deref;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use elsa::sync::FrozenMap;
use rabex::files::SerializedFile;
use rabex::objects::{ClassIdType, PPtr, TypedPPtr};
use rabex::typetree::TypeTreeProvider;

pub trait EnvResolver {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error>;
    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error>;
}

pub struct BaseDirResolver(PathBuf);

impl EnvResolver for BaseDirResolver {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read(self.0.join(path))
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut all = Vec::new();
        for entry in std::fs::read_dir(&self.0)? {
            let entry = entry?;

            if entry.file_type()?.is_dir() {
                continue;
            }

            all.push(entry.path().strip_prefix(&self.0).unwrap().to_owned());
        }
        Ok(all)
    }
}

pub struct Environment<P, R = BaseDirResolver> {
    pub resolver: R,
    pub serialized_files: FrozenMap<PathBuf, Box<(SerializedFile, Vec<u8>)>>,
    pub tpk: P,
}

impl<P: TypeTreeProvider> Environment<P, BaseDirResolver> {
    pub fn new_in(path: impl Into<PathBuf>, tpk: P) -> Self {
        Environment {
            resolver: BaseDirResolver(path.into()),
            serialized_files: Default::default(),
            tpk,
        }
    }
}

impl<R: EnvResolver, P: TypeTreeProvider> Environment<P, R> {
    pub fn load_leaf(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<(SerializedFile, Cursor<Vec<u8>>)> {
        let data = self.resolver.read_path(relative_path.as_ref())?;
        let file = SerializedFile::from_reader(&mut Cursor::new(data.as_slice()))?;
        Ok((file, Cursor::new(data)))
    }

    pub fn load_cached(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<(&SerializedFile, Cursor<&[u8]>)> {
        let (file, data) = self.load_external_file(relative_path.as_ref())?;
        Ok((file, Cursor::new(data)))
    }

    fn load_external_file(&self, path_name: &Path) -> Result<(&SerializedFile, &[u8])> {
        Ok(match self.serialized_files.get(path_name) {
            Some((file, data)) => (file, data.as_slice()),
            None => {
                let data = self
                    .resolver
                    .read_path(Path::new(path_name))
                    .with_context(|| {
                        format!("Cannot read external file {}", path_name.display())
                    })?;
                let serialized = SerializedFile::from_reader(&mut Cursor::new(data.as_slice()))?;
                let items = self
                    .serialized_files
                    .insert(path_name.to_owned(), Box::new((serialized, data)));
                (&items.0, items.1.as_slice())
            }
        })
    }

    pub fn deref_read_untyped<'de, T>(
        &self,
        pptr: PPtr,
        serialized: &SerializedFile,
        serialized_reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: serde::Deserialize<'de>,
    {
        Ok(match pptr.m_FileID {
            0 => pptr
                .deref_local(serialized, &self.tpk)?
                .read(serialized_reader)?,
            file_id => {
                let external = &serialized.m_Externals[file_id as usize - 1];
                let (external_file, external_reader) =
                    self.load_external_file(Path::new(&external.pathName))?;
                let object = pptr
                    .make_local()
                    .deref_local(external_file, &self.tpk)
                    .with_context(|| format!("In external {} {}", file_id, external.pathName))?;
                object.read(&mut Cursor::new(external_reader))?
            }
        })
    }

    pub fn deref_read<'de, T>(
        &self,
        pptr: TypedPPtr<T>,
        serialized: &SerializedFile,
        serialized_reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: ClassIdType + serde::Deserialize<'de>,
    {
        self.deref_read_untyped(pptr.untyped(), serialized, serialized_reader)
    }

    pub fn loaded_files(&mut self) -> impl Iterator<Item = &Path> {
        self.serialized_files.as_mut().keys().map(Deref::deref)
    }
}
