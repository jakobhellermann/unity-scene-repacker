mod completion;
mod locate;
mod logger;
mod utils;

use anyhow::{Context, Result, ensure};
use clap::{Args, CommandFactory as _, Parser};
use clap_complete::ArgValueCompleter;
use indexmap::IndexMap;
use paris::{error, info, success, warn};
use rabex::UnityVersion;
use rabex::files::bundlefile::{self, CompressionType};
use rabex::objects::ClassId;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider as _;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use std::ffi::OsStr;
use std::fs::{DirBuilder, File};
use std::io::{BufWriter, Cursor};
use std::path::PathBuf;
use std::time::Instant;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::{GameFiles, Stats};

use crate::utils::friendly_size;

#[derive(Parser, Debug)]
#[command(version)]
struct Arguments {
    #[clap(flatten)]
    game: GameArgs,

    #[arg(long, default_value = "scene")]
    mode: Mode,

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

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
struct GameArgs {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data1
    #[arg(long)]
    game_dir: Option<PathBuf>,
    #[arg(long, add = ArgValueCompleter::new(completion::complete_steam_game))]
    /// App ID or search term for the steam game to detect
    steam_game: Option<String>,
}

/// What kind of asset bundle to build
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Mode {
    /// Contains filtered 1:1 scenes you can load via `LoadScene`.
    Scene,
    /// A single bundle letting you load specific objects using `LoadAsset`
    Asset,
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
    logger::install();

    if let Err(e) = run() {
        error!("{:?}", e);
        std::process::exit(1);
    }
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
            match locate::find_unity_data_dir(&game_dir) {
                Ok(Some(data_dir)) => data_dir,
                _ => game_dir,
            }
        }
        None => {
            let game = args.game.steam_game.unwrap();
            locate::locate_steam_game(&game)?
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

    let tpk_blob = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let game_files = GameFiles::probe(&game_dir)?;
    let env = Environment::new(game_files, tpk);

    let monobehaviour_node = env
        .tpk
        .get_typetree_node(ClassId::MonoBehaviour, unity_version)
        .unwrap()
        .into_owned();

    let generator = TypeTreeGenerator::new(unity_version, GeneratorBackend::AssetsTools)?;
    generator
        .load_all_dll_in_dir(game_dir.join("Managed"))
        .context("Cannot load game DLLs")?;
    let generator_cache = TypeTreeGeneratorCache::new(generator, monobehaviour_node);

    let mut repack_scenes = unity_scene_repacker::repack_scenes(
        &env,
        &generator_cache,
        preloads,
        matches!(args.mode, Mode::Asset),
        args.disable,
    )?;

    if let Some(parent) = args.output.parent() {
        DirBuilder::new()
            .recursive(true)
            .create(parent)
            .with_context(|| format!("Could not create output directory '{}'", parent.display()))?;
    }

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

    let new_size = match args.mode {
        Mode::Scene => {
            let (stats, header, files) = unity_scene_repacker::pack_to_scene_bundle(
                name,
                &tpk_blob,
                &env.tpk,
                unity_version,
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

            let mut out = BufWriter::new(
                File::create(&args.output).context("Could not write to output file")?,
            );
            bundlefile::write_bundle_iter(
                &header,
                &mut out,
                CompressionType::Lz4hc,
                compression,
                files
                    .into_iter()
                    .map(|(name, file)| Ok((name, Cursor::new(file)))),
            )?;

            out.get_ref().metadata()?.len() as usize
        }
        Mode::Asset => {
            let mut out = BufWriter::new(
                File::create(&args.output).context("Could not write to output file")?,
            );
            let stats = unity_scene_repacker::pack_to_asset_bundle(
                env,
                &mut out,
                name,
                &tpk_blob,
                unity_version,
                repack_scenes,
                compression,
            )?;
            print_stats(&stats);

            out.get_ref().metadata()?.len() as usize
        }
    };

    success!(
        "Repacked '{}' into {} <b>{}</b> <i>({})</i> in {:.2?}",
        name,
        match args.mode {
            Mode::Scene => "scenebundle",
            Mode::Asset => "assetbundle",
        },
        args.output.display(),
        friendly_size(new_size),
        start.elapsed()
    );

    Ok(())
}

fn print_stats(stats: &Stats) {
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
}
