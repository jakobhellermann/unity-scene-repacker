use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rabex::files::bundlefile::BundleFileReader;

use super::Data;

/// A trait abstracting where the game files are read from.
pub trait EnvResolver {
    fn read_path(&self, path: &Path) -> Result<Data, std::io::Error>;
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

/// Extends [`EnvResolver`] by providing the path to the game files.
/// It is expected, that files can be read from there.
pub trait BasedirEnvResolver: EnvResolver {
    fn base_dir(&self) -> &Path;
}

impl EnvResolver for Path {
    fn read_path(&self, path: &Path) -> Result<Data, std::io::Error> {
        let file = File::open(self.join(path))?;
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Data::Mmap(mmap))
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
impl BasedirEnvResolver for Path {
    fn base_dir(&self) -> &Path {
        self
    }
}

impl EnvResolver for PathBuf {
    fn read_path(&self, path: &Path) -> Result<Data, std::io::Error> {
        (**self).read_path(path)
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        (**self).all_files()
    }
}
impl BasedirEnvResolver for PathBuf {
    fn base_dir(&self) -> &Path {
        self
    }
}

impl<T: AsRef<[u8]>> EnvResolver for BundleFileReader<Cursor<T>> {
    fn read_path(&self, path: &Path) -> Result<Data, std::io::Error> {
        let path = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("non-utf8 string"))?;
        let data = self.read_at(path)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File '{path}' does not exist in bundle"),
            )
        })?;

        Ok(Data::InMemory(data))
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        Ok(self
            .files()
            .iter()
            .map(|file| file.path.clone().into())
            .collect())
    }
}
