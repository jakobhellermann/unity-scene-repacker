mod utils;

use anyhow::{Context, Result, ensure};
use clap::{Args, CommandFactory as _, Parser};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use indexmap::IndexMap;
use paris::{error, info, success, warn};
use rabex::UnityVersion;
use rabex::files::bundlefile::{self, CompressionType};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeCache;
use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::time::Instant;
use utils::TempDir;

use crate::utils::friendly_size;

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct GameArgs {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data1
    #[arg(long)]
    game_dir: Option<PathBuf>,
    #[arg(long, add = ArgValueCompleter::new(complete_steam_game))]
    /// App ID or search term for the steam game to detect
    steam_game: Option<String>,
}

fn complete_steam_game(current: &OsStr) -> Vec<CompletionCandidate> {
    fn complete_steam_game_inner(_: &OsStr) -> Result<Vec<CompletionCandidate>> {
        let steam = steamlocate::SteamDir::locate()?;

        let mut candidates = Vec::new();

        for library in steam.libraries()?.filter_map(Result::ok) {
            for app in library.apps().filter_map(Result::ok) {
                let app_dir = library.resolve_app_dir(&app);
                let Some(_) = find_data_dir(&app_dir).transpose() else {
                    continue;
                };
                let name = app
                    .name
                    .map(OsString::from)
                    .unwrap_or(Path::new(&app.install_dir).file_name().unwrap().to_owned());
                candidates
                    .push(CompletionCandidate::new(name).help(Some(app.app_id.to_string().into())));
            }
        }

        Ok(candidates)
    }

    complete_steam_game_inner(current).unwrap_or_default()
}

#[derive(Parser, Debug)]
#[command(version)]
struct Arguments {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data1
    #[clap(flatten)]
    game: GameArgs,
    /// Path to JSON file, containing a map of scene name to a list of gameobject paths to include
    /// ```json
    /// {
    ///   "Fungus1_12": [
    ///     "simple_grass",
    ///     "green_grass_2",
    ///   ],
    ///   "White_Palace_01": [
    ///     "WhiteBench",
    ///   ]
    /// }
    /// ```
    #[arg(long)]
    objects: PathBuf,
    /// When true, all gameobjects in the scene will start out disabled
    #[arg(long, default_value = "false")]
    disable: bool,
    /// Compression level to apply
    #[arg(long, default_value = "lzma")]
    compression: Compression,
    #[arg(long, short = 'o', default_value = "out.unity3d")]
    output: PathBuf,

    /// Name to give the assetbundle. Should be unique for your game.
    #[arg(long)]
    bundle_name: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Compression {
    None = 0,
    Lzma = 1,
    // Lz4 = 2,
    /// Best compression at the cost of speed
    Lz4hc = 3,
    // Lzham = 4,
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    clap_complete::CompleteEnv::with_factory(Arguments::command).complete();

    if let Err(e) = run() {
        error!("{:?}", e);
        std::process::exit(1);
    }
}

fn locate(game: &str) -> Result<PathBuf> {
    let steam = steamlocate::SteamDir::locate()?;

    let game = game.to_ascii_lowercase();

    let (app, library) = if let Ok(app_id) = game.parse() {
        steam
            .find_app(app_id)?
            .with_context(|| format!("Could not locate game with app id {app_id}"))?
    } else {
        steam
            .libraries()?
            .filter_map(Result::ok)
            .find_map(|library| {
                let app = library.apps().filter_map(Result::ok).find(|app| {
                    let name = app.name.as_ref().unwrap_or(&app.install_dir);
                    name.to_ascii_lowercase().contains(&game)
                })?;
                Some((app, library))
            })
            .with_context(|| format!("Didn't find any steam game matching '{game}'"))?
    };

    let install_dir = library.resolve_app_dir(&app);
    let name = app.name.as_ref().unwrap_or(&app.install_dir);
    info!("Detected game '{}' at '{}'", name, install_dir.display());

    find_data_dir(&install_dir)?.with_context(|| {
        format!(
            "Did not find unity 'game_Data' directory in '{}'. Is {} a unity game?",
            install_dir.display(),
            name
        )
    })
}

fn find_data_dir(install_dir: &Path) -> Result<Option<PathBuf>> {
    Ok(std::fs::read_dir(install_dir)?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .path()
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with("_Data"))
                && entry.file_type().is_ok_and(|ty| ty.is_dir())
        })
        .map(|entry| entry.path()))
}

fn run() -> Result<()> {
    let args = Arguments::parse();

    let game_dir = match args.game.game_dir {
        Some(game_dir) => {
            ensure!(
                game_dir.exists(),
                "Game directory '{}' does not exist",
                game_dir.display()
            );
            match find_data_dir(&game_dir) {
                Ok(Some(data_dir)) => data_dir,
                _ => game_dir,
            }
        }
        None => {
            let game = args.game.steam_game.unwrap();
            locate(&game)?
        }
    };
    let unity_version: UnityVersion = "2020.2.2f1".parse().unwrap();

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let start = Instant::now();

    let preloads = std::fs::read_to_string(&args.objects)
        .with_context(|| format!("couldn't find object json '{}'", args.objects.display()))?;
    let preloads: IndexMap<String, Vec<String>> =
        json5::from_str(&preloads).context("error parsing the objects json")?;

    let obj_count = preloads
        .iter()
        .map(|(_, objects)| objects.len())
        .sum::<usize>();
    info!("Repacking {obj_count} objects in {} scenes", preloads.len());

    let tpk = TpkTypeTreeBlob::embedded();
    let tt = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let temp_dir = TempDir::named_in_tmp("unity-scene-repacker")?;

    let mut repack_scenes =
        unity_scene_repacker::repack_scenes(&game_dir, preloads, &tpk, &temp_dir.dir)?;

    if let Some(parent) = args.output.parent() {
        DirBuilder::new()
            .recursive(true)
            .create(parent)
            .with_context(|| format!("Could not create output directory '{}'", parent.display()))?;
    }

    let mut out =
        BufWriter::new(File::create(&args.output).context("Could not write to output file")?);

    let compression = match args.compression {
        Compression::None => CompressionType::None,
        Compression::Lzma => CompressionType::Lzma,
        // Compression::Lz4 => CompressionType::Lz4,
        Compression::Lz4hc => CompressionType::Lz4hc,
        // Compression::Lzham => CompressionType::Lzham,
    };

    let name = match &args.bundle_name {
        Some(name) => name,
        None => {
            let name = args
                .output
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("unity-scene-repacker-bundle");
            warn!(
                "Did not specify --bundle-name, falling back to '{name}'. This might conflict with other loaded asset bundles"
            );
            name
        }
    };

    let (stats, header, files) = unity_scene_repacker::repack_bundle(
        name,
        &tpk,
        &tt,
        unity_version,
        args.disable,
        repack_scenes.as_mut_slice(),
    )
    .context("trying to repack bundle")?;

    info!(
        "Pruned {} -> <b>{}</b> objects",
        stats.objects_before, stats.objects_after
    );
    info!(
        "{} -> <b>{}</b> raw size",
        friendly_size(stats.size_before),
        friendly_size(stats.size_after)
    );
    println!();

    bundlefile::write_bundle_iter(
        &header,
        &mut out,
        CompressionType::Lz4hc,
        compression,
        files
            .into_iter()
            .map(|(name, file)| Ok((name, Cursor::new(file)))),
    )?;

    success!(
        "Repacked '{}' into <b>{}</b> <i>({})</i> in {:.2?}",
        name,
        args.output.display(),
        friendly_size(out.get_ref().metadata()?.len() as usize),
        start.elapsed()
    );

    Ok(())
}
