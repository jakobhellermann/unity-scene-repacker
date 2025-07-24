use std::io::{Cursor, Read, Seek};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use elsa::sync::FrozenMap;
use rabex::UnityVersion;
use rabex::files::SerializedFile;
use rabex::objects::{ClassId, PPtr, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;

pub mod game_files;
mod resolver;

pub use resolver::EnvResolver;
use typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};

use crate::GameFiles;
use crate::env::resolver::BasedirEnvResolver;
use crate::typetree_generator_cache::TypeTreeGeneratorCache;

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

impl<P: TypeTreeProvider> Environment<GameFiles, P> {
    pub fn new_in(path: impl AsRef<Path>, tpk: P) -> Result<Self> {
        Ok(Environment {
            resolver: GameFiles::probe(path.as_ref())?,
            tpk,
            serialized_files: Default::default(),
            typetree_generator: TypeTreeGeneratorCache::empty(),
            unity_version: OnceLock::new(),
        })
    }
}

impl<R: BasedirEnvResolver, P: TypeTreeProvider> Environment<R, P> {
    /// Initializes [`Environment::typetree_generator`] from the `Managed` DLLs.
    /// Requires `libTypeTreeGenerator.so`/`TypeTreeGenerator.dll` next to the executing binary.
    pub fn load_typetree_generator(&mut self, backend: GeneratorBackend) -> Result<()> {
        let unity_version = self.unity_version()?;
        let generator = TypeTreeGenerator::new_lib_next_to_exe(unity_version, backend)?;
        generator.load_all_dll_in_dir(self.resolver.base_dir().join("Managed"))?;
        let base_node = self
            .tpk
            .get_typetree_node(ClassId::MonoBehaviour, unity_version)
            .expect("missing MonoBehaviour class");
        self.typetree_generator = TypeTreeGeneratorCache::new(generator, base_node.into_owned());

        Ok(())
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

    pub fn loaded_files(&mut self) -> impl Iterator<Item = &Path> {
        self.serialized_files.as_mut().keys().map(Deref::deref)
    }
}
