mod completion;
mod locate;
mod logger;
mod utils;

#[cfg(feature = "python-module")]
mod py;

use anyhow::{Context, Result, bail, ensure};
use clap::{Args, CommandFactory as _, Parser};
use clap_complete::ArgValueCompleter;
use indexmap::{IndexMap, IndexSet};
use paris::{error, info, success, warn};
use rabex::files::bundlefile::CompressionType;
use rabex::objects::ClassId;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider as _;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::{GameFiles, RepackSettings, Stats};

use crate::utils::friendly_size;

#[derive(Parser, Debug)]
#[command(version)]
struct Arguments {
    #[clap(flatten)]
    game: GameArgs,
    #[clap(flatten)]
    repack: RepackArgs,
    #[clap(flatten)]
    output: OutputArgs,
}

#[derive(Args, Debug)]
#[group(required = true, multiple = false)]
#[clap(next_help_heading = "Game options")]
struct GameArgs {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data
    #[arg(long)]
    game_dir: Option<PathBuf>,
    #[arg(long, add = ArgValueCompleter::new(completion::complete_steam_game))]
    /// App ID or search term for the steam game to detect
    steam_game: Option<String>,
}

#[derive(Args, Debug)]
#[clap(next_help_heading = "Repack options")]
struct RepackArgs {
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
    #[arg(alias = "objects")]
    scene_objects: Option<PathBuf>,

    /// Path to JSON file, containing a map of C# type to monobehaviour names.
    /// Useful for scriptable objects etc., which do not exist in the transform hierarchy.
    /// ```json
    /// {
    ///   "FXDealerMaterialTag": ["[FXDealer] 0_YeeAttack _PostureDecrease"]
    /// }
    /// ```
    #[arg(long)]
    extra_objects: Option<PathBuf>,
}

#[derive(Args, Debug)]
#[clap(next_help_heading = "Output options")]
struct OutputArgs {
    #[arg(long, default_value = "scene")]
    mode: Mode,

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

/// What kind of asset bundle to build
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Mode {
    /// A scene asset bundle, containing the original scenes filtered. Load using `LoadScene`.
    Scene,
    /// An asset bundle containing individual assets you can load using `LoadAsset`.
    /// The objects are copied from the original level files.
    Asset,
    /// An asset bundle containing individual assets you can load using `LoadAsset`.
    /// The bundle is completely empty, and only references the original game level files.
    AssetShallow,
}
impl Mode {
    fn needs_typetree_generator(&self) -> bool {
        matches!(self, Mode::Asset)
    }
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

pub fn main(args: Vec<OsString>, libs_dir: Option<&Path>) {
    if clap_complete::CompleteEnv::with_factory(Arguments::command)
        .try_complete(&args, std::env::current_dir().ok().as_deref())
        .unwrap_or_else(|e| e.exit())
    {
        std::process::exit(0);
    }

    logger::install();

    if let Err(e) = run(args, libs_dir) {
        error!("{:?}", e);
        std::process::exit(1);
    }
}

fn run(args: Vec<OsString>, libs_dir: Option<&Path>) -> Result<()> {
    let args = Arguments::parse_from(args);

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

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let start = Instant::now();

    let scene_objects = args
        .repack
        .scene_objects
        .as_ref()
        .map(|path| -> Result<IndexMap<String, Vec<String>>> {
            let preloads = std::fs::read_to_string(path).with_context(|| {
                format!("couldn't find scene objects json '{}'", path.display())
            })?;
            json5::from_str(&preloads).context("error parsing the scene objects json")
        })
        .transpose()?
        .unwrap_or_default();
    let extra_objects = args
        .repack
        .extra_objects
        .as_ref()
        .map(|path| -> Result<IndexMap<String, IndexSet<String>>> {
            let preloads = std::fs::read_to_string(path).with_context(|| {
                format!(
                    "couldn't find extra monobehaviour json '{}'",
                    path.display()
                )
            })?;
            json5::from_str(&preloads).context("error parsing the extra monobehaviours json")
        })
        .transpose()?
        .unwrap_or_default();

    if !scene_objects.is_empty() {
        let obj_count = scene_objects
            .iter()
            .map(|(_, objects)| objects.len())
            .sum::<usize>();
        info!(
            "Repacking {obj_count} objects in {} scenes",
            scene_objects.len()
        );
    }
    if !extra_objects.is_empty() {
        let obj_count = extra_objects
            .iter()
            .map(|(_, objects)| objects.len())
            .sum::<usize>();
        info!(
            "Repacking {obj_count} extra monobehaviour{}",
            if obj_count == 1 { "" } else { "s" }
        );
    }
    let repack_settings = RepackSettings {
        scene_objects,
        extra_objects,
    };

    if repack_settings.is_empty() {
        bail!("Nothing to repack specified. See `--help` for possible repack options.")
    }

    let tpk_blob = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let game_files = GameFiles::probe(&game_dir)?;
    let mut env = Environment::new(game_files, tpk);
    let unity_version = env.unity_version()?;

    if args.output.mode.needs_typetree_generator() {
        let generator = match libs_dir {
            Some(lib_path) => TypeTreeGenerator::new_lib_in(
                lib_path,
                unity_version,
                GeneratorBackend::AssetsTools,
            )?,
            None => TypeTreeGenerator::new_lib_next_to_exe(
                unity_version,
                GeneratorBackend::AssetsTools,
            )?,
        };
        generator
            .load_all_dll_in_dir(env.resolver.game_dir.join("Managed"))
            .context("Cannot load game DLLs")?;
        let monobehaviour_node = env
            .tpk
            .get_typetree_node(ClassId::MonoBehaviour, unity_version)
            .unwrap()
            .into_owned();
        env.typetree_generator = TypeTreeGeneratorCache::new(generator, monobehaviour_node);
    };

    let name = match &args.output.bundle_name {
        Some(name) => name,
        None => {
            let name = args
                .output
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

    let compression = match args.output.compression {
        Compression::None => CompressionType::None,
        Compression::Lzma => CompressionType::Lzma,
        // Compression::Lz4 => CompressionType::Lz4,
        Compression::Lz4hc => CompressionType::Lz4hc,
        // Compression::Lzham => CompressionType::Lzham,
    };

    if let Mode::AssetShallow = args.output.mode {
        let mut out = BufWriter::new(
            File::create(&args.output.output).context("Could not write to output file")?,
        );

        unity_scene_repacker::pack_to_shallow_asset_bundle(
            &env,
            &mut out,
            name,
            repack_settings,
            compression,
        )?;

        let new_size = out.get_ref().metadata()?.len() as usize;

        success!(
            "Repacked '{}' into shallow asset bundle <b>{}</b> <i>({})</i> in {:.2?}",
            name,
            args.output.output.display(),
            friendly_size(new_size),
            start.elapsed()
        );

        return Ok(());
    }

    let (mut repack_scenes, extra_objects) = unity_scene_repacker::repack_scenes(
        &env,
        repack_settings,
        matches!(args.output.mode, Mode::Asset),
        args.output.disable,
    )?;

    if let Some(parent) = args.output.output.parent() {
        DirBuilder::new()
            .recursive(true)
            .create(parent)
            .with_context(|| format!("Could not create output directory '{}'", parent.display()))?;
    }

    let new_size = match args.output.mode {
        Mode::Scene => {
            let mut out = BufWriter::new(
                File::create(&args.output.output).context("Could not write to output file")?,
            );

            let stats = unity_scene_repacker::pack_to_scene_bundle(
                &mut out,
                name,
                &tpk_blob,
                &env.tpk,
                unity_version,
                repack_scenes.as_mut_slice(),
                compression,
            )
            .context("trying to repack bundle")?;

            print_stats(&stats, args.repack.scene_objects.is_some());

            out.get_ref().metadata()?.len() as usize
        }
        Mode::Asset => {
            let mut out = BufWriter::new(
                File::create(&args.output.output).context("Could not write to output file")?,
            );
            let stats = unity_scene_repacker::pack_to_asset_bundle(
                &env,
                &mut out,
                name,
                &tpk_blob,
                repack_scenes,
                extra_objects,
                compression,
            )?;
            print_stats(&stats, args.repack.scene_objects.is_some());

            out.get_ref().metadata()?.len() as usize
        }
        Mode::AssetShallow => todo!(),
    };

    success!(
        "Repacked '{}' into {} <b>{}</b> <i>({})</i> in {:.2?}",
        name,
        match args.output.mode {
            Mode::Scene => "scenebundle",
            Mode::Asset => "assetbundle",
            Mode::AssetShallow => todo!(),
        },
        args.output.output.display(),
        friendly_size(new_size),
        start.elapsed()
    );

    Ok(())
}

fn print_stats(stats: &Stats, has_scene_objects: bool) {
    if has_scene_objects {
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
}
