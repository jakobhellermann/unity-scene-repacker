use std::fs::File;
use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use memmap2::Mmap;
use rabex::files::bundlefile::{BundleFileReader, ExtractionConfig};

use crate::env::EnvResolver;

pub enum GameFiles {
    Directory(PathBuf),
    Bundle {
        game_dir: PathBuf,
        bundle: Box<BundleFileReader<Cursor<Mmap>>>,
    },
}

impl GameFiles {
    pub fn game_dir(&self) -> &Path {
        match self {
            GameFiles::Directory(base) => base,
            GameFiles::Bundle { game_dir, .. } => game_dir,
        }
    }
    pub fn probe(game_dir: &Path) -> Result<GameFiles> {
        ensure!(
            game_dir.exists(),
            "Game Directory '{}' does not exist",
            game_dir.display()
        );

        let bundle_path = game_dir.join("data.unity3d");
        if bundle_path.exists() {
            let reader = unsafe { Mmap::map(&File::open(&bundle_path)?) }?;
            let bundle =
                BundleFileReader::from_reader(Cursor::new(reader), &ExtractionConfig::default())?;

            Ok(GameFiles::Bundle {
                game_dir: game_dir.to_owned(),
                bundle: Box::new(bundle),
            })
        } else {
            Ok(GameFiles::Directory(game_dir.to_owned()))
        }
    }
}

impl EnvResolver for GameFiles {
    fn read_path(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        match self {
            GameFiles::Directory(base) => base.read_path(path),
            GameFiles::Bundle { bundle, game_dir } => {
                if let Ok(suffix) = path.strip_prefix("Library") {
                    let resource_path = game_dir.join("Resources").join(suffix);
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
        match self {
            GameFiles::Directory(base) => base.all_files(),
            GameFiles::Bundle { bundle, .. } => bundle.all_files(),
        }
    }
}
