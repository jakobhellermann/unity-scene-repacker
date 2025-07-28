use std::io::{Cursor, Read, Seek};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use elsa::sync::FrozenMap;
use rabex::UnityVersion;
use rabex::files::SerializedFile;
use rabex::files::bundlefile::BundleFileReader;
use rabex::objects::{PPtr, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;

use crate::GameFiles;
use crate::typetree_generator_cache::TypeTreeGeneratorCache;

pub trait EnvResolver {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error>;
    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error>;

    fn serialized_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        Ok(self
            .all_files()?
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?;
                let is_level = name
                    .strip_prefix("level")
                    .and_then(|x| x.parse::<usize>().ok())
                    .is_some();

                let is_serialized = is_level
                    || path.extension().is_some_and(|e| e == "assets")
                    || name == "globalgamemanagers";

                is_serialized.then_some(path)
            })
            .collect())
    }

    fn level_files(&self) -> Result<Vec<usize>, std::io::Error> {
        Ok(self
            .all_files()?
            .iter()
            .filter_map(|path| path.file_name()?.to_str())
            .filter_map(|path| {
                let index = path.strip_prefix("level")?;
                index.parse::<usize>().ok()
            })
            .collect())
    }
}

impl<T: AsRef<[u8]>> EnvResolver for BundleFileReader<Cursor<T>> {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        let path = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("non-utf8 string"))?;
        let data = self.read_at(path)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File '{path}' does not exist in bundle"),
            )
        })?;

        Ok(data)
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        Ok(self
            .files()
            .iter()
            .map(|file| file.path.clone().into())
            .collect())
    }
}

impl EnvResolver for Path {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read(self.join(path))
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut all = Vec::new();
        for entry in std::fs::read_dir(self)? {
            let entry = entry?;

            if entry.file_type()?.is_dir() {
                continue;
            }

            all.push(entry.path().strip_prefix(self).unwrap().to_owned());
        }
        Ok(all)
    }
}
impl EnvResolver for PathBuf {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        (**self).read_path(path)
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        (**self).all_files()
    }
}

pub struct Environment<R = GameFiles, P = TypeTreeCache<TpkTypeTreeBlob>> {
    pub resolver: R,
    pub tpk: P,
    pub serialized_files: FrozenMap<PathBuf, Box<(SerializedFile, Vec<u8>)>>,
    pub typetree_generator: TypeTreeGeneratorCache,
    unity_version: OnceLock<UnityVersion>,
}

impl<R, P> Environment<R, P> {
    pub fn new(resolver: R, tpk: P) -> Self {
        Environment {
            resolver,
            tpk,
            serialized_files: Default::default(),
            typetree_generator: TypeTreeGeneratorCache::empty(),
            unity_version: OnceLock::new(),
        }
    }
}

impl<P: TypeTreeProvider> Environment<PathBuf, P> {
    pub fn new_in(path: impl Into<PathBuf>, tpk: P) -> Self {
        Environment {
            resolver: path.into(),
            tpk,
            serialized_files: Default::default(),
            typetree_generator: TypeTreeGeneratorCache::empty(),
            unity_version: OnceLock::new(),
        }
    }
}

impl<R: EnvResolver, P: TypeTreeProvider> Environment<R, P> {
    pub fn unity_version(&self) -> Result<UnityVersion> {
        match self.unity_version.get() {
            Some(unity_version) => Ok(*unity_version),
            None => {
                let (ggm, _) = self.load_cached("globalgamemanagers")?;
                let unity_version = ggm.m_UnityVersion.expect("missing unity version");
                let _ = self.unity_version.set(unity_version);
                Ok(unity_version)
            }
        }
    }

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
        file: &SerializedFile,
        reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: serde::Deserialize<'de>,
    {
        Ok(match pptr.m_FileID {
            0 => pptr.deref_local(file, &self.tpk)?.read(reader)?,
            file_id => {
                let external = &file.m_Externals[file_id as usize - 1];
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
        file: &SerializedFile,
        reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: serde::Deserialize<'de>,
    {
        self.deref_read_untyped(pptr.untyped(), file, reader)
    }

    pub fn loaded_files(&mut self) -> impl Iterator<Item = &Path> {
        self.serialized_files.as_mut().keys().map(Deref::deref)
    }
}
