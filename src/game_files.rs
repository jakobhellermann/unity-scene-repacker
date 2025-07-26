use std::fs::File;
use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use memmap2::Mmap;
use rabex::files::bundlefile::{BundleFileReader, ExtractionConfig};

use crate::env::EnvResolver;

pub struct GameFiles {
    pub game_dir: PathBuf,
    pub level_files: LevelFiles,
}
pub enum LevelFiles {
    Unpacked,
    Packed(Box<BundleFileReader<Cursor<Mmap>>>),
}

impl GameFiles {
    pub fn probe(game_dir: &Path) -> Result<GameFiles> {
        ensure!(
            game_dir.exists(),
            "Game Directory '{}' does not exist",
            game_dir.display()
        );

        let bundle_path = game_dir.join("data.unity3d");
        let level_files = if bundle_path.exists() {
            let reader = unsafe { Mmap::map(&File::open(&bundle_path)?)? };
            let bundle =
                BundleFileReader::from_reader(Cursor::new(reader), &ExtractionConfig::default())?;

            LevelFiles::Packed(Box::new(bundle))
        } else {
            LevelFiles::Unpacked
        };

        Ok(GameFiles {
            game_dir: game_dir.to_owned(),
            level_files,
        })
    }

    pub fn read(&self, filename: &str) -> Result<Data, std::io::Error> {
        match &self.level_files {
            LevelFiles::Unpacked => {
                let path = self.game_dir.join(filename);
                let file = File::open(path)?;
                let mmap = unsafe { Mmap::map(&file)? };
                Ok(Data::Mmap(mmap))
            }
            LevelFiles::Packed(bundle) => {
                let data = bundle.read_at(filename)?.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "File not found in bundle")
                })?;
                Ok(Data::InMemory(data))
            }
        }
    }
}
pub enum Data {
    InMemory(Vec<u8>),
    Mmap(Mmap),
}
impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        match self {
            Data::InMemory(data) => data.as_slice(),
            Data::Mmap(mmap) => mmap.as_ref(),
        }
    }
}

impl EnvResolver for GameFiles {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        match &self.level_files {
            LevelFiles::Unpacked => self.game_dir.read_path(path),
            LevelFiles::Packed(bundle) => {
                if let Ok(suffix) = path.strip_prefix("Library") {
                    let resource_path = self.game_dir.join("Resources").join(suffix);
                    match std::fs::read(resource_path) {
                        Ok(val) => return Ok(val),
                        Err(e) if e.kind() == ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
                bundle.read_path(path)
            }
        }
    }

    fn all_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        match &self.level_files {
            LevelFiles::Unpacked => self.game_dir.all_files(),
            LevelFiles::Packed(bundle) => bundle.all_files(),
        }
    }
}
