use std::io::{Cursor, Read, Seek};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use elsa::sync::FrozenMap;
use rabex::UnityVersion;
use rabex::files::SerializedFile;
use rabex::files::serializedfile::ObjectRef;
use rabex::objects::{PPtr, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;

pub mod game_files;
mod resolver;

pub use resolver::EnvResolver;

use crate::GameFiles;
use crate::typetree_generator_cache::TypeTreeGeneratorCache;
use crate::unity::types::{BuildSettings, MonoBehaviour};

pub enum Data {
    InMemory(Vec<u8>),
    Mmap(memmap2::Mmap),
}
impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        match self {
            Data::InMemory(data) => data.as_slice(),
            Data::Mmap(mmap) => mmap.as_ref(),
        }
    }
}

pub struct Environment<R = GameFiles, P = TypeTreeCache<TpkTypeTreeBlob>> {
    pub resolver: R,
    pub tpk: P,
    pub serialized_files: FrozenMap<PathBuf, Box<(SerializedFile, Data)>>,
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

    pub fn build_settings(&self) -> Result<BuildSettings> {
        let (ggm, mut ggm_reader) = self.load_cached("globalgamemanagers")?;
        let build_settings = ggm
            .find_object_of::<BuildSettings>(&self.tpk)?
            .unwrap()
            .read(&mut ggm_reader)?;
        Ok(build_settings)
    }

    pub fn load_leaf(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<(SerializedFile, Cursor<Data>)> {
        let data = self.resolver.read_path(relative_path.as_ref())?;
        let file = SerializedFile::from_reader(&mut Cursor::new(data.as_ref()))?;
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
            Some((file, data)) => (file, data.as_ref()),
            None => {
                let data = self
                    .resolver
                    .read_path(Path::new(path_name))
                    .with_context(|| {
                        format!("Cannot read external file {}", path_name.display())
                    })?;
                let serialized = SerializedFile::from_reader(&mut Cursor::new(data.as_ref()))?;
                let items = self
                    .serialized_files
                    .insert(path_name.to_owned(), Box::new((serialized, data)));
                (&items.0, items.1.as_ref())
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

    pub fn deref_read_monobehaviour_untyped<'de, T>(
        &self,
        pptr: PPtr,
        serialized: &SerializedFile,
        serialized_reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: serde::Deserialize<'de>,
    {
        match pptr.m_FileID {
            0 => {
                let mb_obj = pptr.deref_local(serialized, &self.tpk)?;
                let data = self
                    .read_monobehaviour_data(&mb_obj, serialized, serialized_reader)?
                    .read(serialized_reader)?;
                Ok(data)
            }
            file_id => {
                let external = &serialized.m_Externals[file_id as usize - 1];
                let (external_file, external_reader) =
                    self.load_external_file(Path::new(&external.pathName))?;
                let external_reader = &mut Cursor::new(external_reader);
                let mb_obj = pptr
                    .make_local()
                    .deref_local::<MonoBehaviour>(external_file, &self.tpk)
                    .with_context(|| format!("In external {} {}", file_id, external.pathName))?;

                let data = self
                    .read_monobehaviour_data(&mb_obj, external_file, external_reader)?
                    .read(external_reader)?;
                Ok(data)
            }
        }
    }

    pub fn deref_data<'a, Reader: 'a>(
        &'a self,
        pptr: PPtr,
        serialized: &'a SerializedFile,
        serialized_reader: Reader,
    ) -> Result<(&'a SerializedFile, MaybeCached<'a, Reader>)> {
        match pptr.m_FileID {
            0 => Ok((serialized, MaybeCached::A(serialized_reader))),
            file_id => {
                let external = &serialized.m_Externals[file_id as usize - 1];
                let (external_file, external_reader) =
                    self.load_external_file(Path::new(&external.pathName))?;
                Ok((external_file, MaybeCached::B(Cursor::new(external_reader))))
            }
        }
    }

    pub fn deref_read_monobehaviour<'de, T>(
        &self,
        pptr: TypedPPtr<T>,
        serialized: &SerializedFile,
        serialized_reader: &mut (impl Read + Seek),
    ) -> Result<T>
    where
        T: serde::Deserialize<'de>,
    {
        self.deref_read_monobehaviour_untyped(pptr.untyped(), serialized, serialized_reader)
    }

    pub fn loaded_files(&mut self) -> impl Iterator<Item = &Path> {
        self.serialized_files.as_mut().keys().map(Deref::deref)
    }

    fn read_monobehaviour_data<'a, T>(
        &'a self,
        mb_obj: &ObjectRef<'a, MonoBehaviour>,
        file: &SerializedFile,
        reader: &mut (impl Read + Seek),
    ) -> Result<ObjectRef<'a, T>> {
        let mb = mb_obj.read(reader)?;
        let script = self.deref_read(mb.m_Script, file, reader)?;
        let ty = self
            .typetree_generator
            .generate(&script.m_AssemblyName, &script.full_name())?;
        Ok(mb_obj.with_typetree::<T>(ty))
    }
}

pub enum MaybeCached<'a, A> {
    A(A),
    B(Cursor<&'a [u8]>),
}

impl<A> Read for MaybeCached<'_, A>
where
    A: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            MaybeCached::A(a) => a.read(buf),
            MaybeCached::B(b) => b.read(buf),
        }
    }
}

impl<A> Seek for MaybeCached<'_, A>
where
    A: Seek,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            MaybeCached::A(a) => a.seek(pos),
            MaybeCached::B(b) => b.seek(pos),
        }
    }
}
