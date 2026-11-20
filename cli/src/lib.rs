mod check_scripts;
mod completion;
mod locate;
mod logger;
mod utils;

#[cfg(feature = "python-module")]
mod py;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory as _, Parser};
use clap_complete::ArgValueCompleter;
use indexmap::{IndexMap, IndexSet};
use paris::{error, info, success, warn};
use rabex::files::bundlefile::CompressionType;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rabex_env::Environment;
use rabex_env::resolver::GameFiles;
use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};
use std::io::{BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;
use unity_scene_repacker::dependency_addressables::Dependencies;
use unity_scene_repacker::{RepackSettings, Stats};

use crate::utils::friendly_size;

#[derive(Parser, Debug)]
#[command(
    version,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
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
pub(crate) struct GameArgs {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data
    #[arg(long)]
    pub game_dir: Option<PathBuf>,
    #[arg(long, add = ArgValueCompleter::new(completion::complete_steam_game))]
    /// App ID or search term for the steam game to detect
    pub steam_game: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Check whether the scripts used in a repacked bundle exist with the same fields in another game.
    CheckScripts(check_scripts::CheckScriptsArgs),
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
    #[arg(long, default_value = "lz4")]
    compression: Compression,

    #[arg(long, short = 'o', default_value = "out.unity3d")]
    output: PathBuf,

    /// Name to give the assetbundle. Should be unique for your game.
    #[arg(long)]
    bundle_name: Option<String>,

    /// When set, write the resulting bundle with typetree information embedded.
    ///
    /// This lets unity load asset bundles for different unity versions, as long as they are similar enough.
    /// Larger version differences might not work due to format changes to e.g. shaders, materials and animations.
    #[arg(long)]
    write_typetrees: bool,

    /// When enabled, the game files the bundle depends on get copied to `deps/` next to the output, and
    /// an Addressables catalog `<bundle name>.catalog.bin` is written referencing the repacked objects and their dependencies.
    ///
    /// The catalog loads bundles from the Addressables runtime property `{UnitySceneRepacker.<bundle name>}`.
    /// Implies `--write-typetrees`.
    #[arg(long)]
    emit_dependency_addressables: bool,
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

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Compression {
    None = 0,
    Lzma = 1,
    /// Fast compression, larger output than lz4hc
    Lz4 = 2,
    /// Best compression at the cost of speed
    Lz4hc = 3,
    // Lzham = 4,
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

pub fn main(args: Vec<OsString>) {
    if clap_complete::CompleteEnv::with_factory(Arguments::command)
        .try_complete(&args, std::env::current_dir().ok().as_deref())
        .unwrap_or_else(|e| e.exit())
    {
        std::process::exit(0);
    }

    logger::install();

    if let Err(e) = run(args) {
        error!("{:?}", e);
        std::process::exit(1);
    }
}

fn run(args: Vec<OsString>) -> Result<()> {
    let args = Arguments::parse_from(args);

    if let Some(Command::CheckScripts(args)) = args.command {
        return check_scripts::run(args);
    }

    let game_dir = resolve_game_dir(args.game.game_dir, args.game.steam_game)?;

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
        strict: false,
    };

    if repack_settings.is_empty() {
        bail!("Nothing to repack specified. See `--help` for possible repack options.")
    }

    let tpk_blob = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let game_files = GameFiles::probe(&game_dir)?;
    let env = Environment::new(game_files, tpk);

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
        Compression::Lz4 => CompressionType::Lz4,
        Compression::Lz4hc => CompressionType::Lz4hc,
        // Compression::Lzham => CompressionType::Lzham,
    };

    if args.output.write_typetrees && !matches!(args.output.mode, Mode::Asset) {
        bail!("--write-typetrees is only supported with --mode asset");
    }
    if args.output.emit_dependency_addressables && !matches!(args.output.mode, Mode::Asset) {
        bail!("--emit-dependency-addressables is only supported with --mode asset");
    }

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

    let enable_typetree = args.output.write_typetrees || args.output.emit_dependency_addressables;
    let dependencies = match args.output.emit_dependency_addressables {
        true => Some(Dependencies::new(&env, &repack_scenes)?),
        false => None,
    };

    let new_size = match args.output.mode {
        Mode::Scene => {
            let mut out = BufWriter::new(
                File::create(&args.output.output).context("Could not write to output file")?,
            );

            let stats = unity_scene_repacker::pack_to_scene_bundle(
                &env,
                &mut out,
                name,
                &tpk_blob,
                repack_scenes.as_mut_slice(),
                compression,
                enable_typetree,
                None,
                &[],
            )
            .context("trying to repack bundle")?;

            print_stats(&stats, args.repack.scene_objects.is_some());

            out.get_ref().metadata()?.len() as usize
        }
        Mode::Asset => {
            let mut out = BufWriter::new(
                File::create(&args.output.output).context("Could not write to output file")?,
            );
            let out_dir = args.output.output.parent();
            let (stats, dependency_result) = rayon::join(
                || -> Result<()> {
                    let stats = unity_scene_repacker::pack_to_asset_bundle(
                        &env,
                        &mut out,
                        name,
                        &tpk_blob,
                        repack_scenes,
                        extra_objects,
                        compression,
                        enable_typetree,
                        dependencies.as_ref(),
                        &[],
                    )?;
                    print_stats(&stats, args.repack.scene_objects.is_some());
                    Ok(())
                },
                || -> Result<()> {
                    if let Some(dependencies) = &dependencies {
                        let out_dir = out_dir.context("--output has no parent directory")?;
                        let start = Instant::now();
                        dependencies.write_bundles(&env, out_dir, compression)?;
                        info!(
                            "Wrote {} dependency bundles in {:.2?}",
                            dependencies.files().len(),
                            start.elapsed()
                        );
                    }
                    Ok(())
                },
            );
            stats?;
            dependency_result?;
            out.flush()?;

            if let Some(dependencies) = &dependencies {
                write_catalog(&env, dependencies, &args.output.output, name)?;
            }

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

fn write_catalog(
    env: &Environment,
    dependencies: &Dependencies,
    output: &Path,
    bundle_name: &str,
) -> Result<()> {
    let file_name = output.file_name().context("--output has no file name")?;
    let file_name = file_name.to_str().context("--output is not valid UTF-8")?;
    let catalog = dependencies.catalog(env, file_name, bundle_name)?;

    std::fs::write(
        output.with_file_name(format!("{bundle_name}.catalog.bin")),
        catalog.to_binary()?,
    )?;

    Ok(())
}

pub(crate) fn resolve_game_dir(
    game_dir: Option<PathBuf>,
    steam_game: Option<String>,
) -> Result<PathBuf> {
    match (game_dir, steam_game) {
        (Some(game_dir), _) => GameFiles::probe_dir(&game_dir),
        (None, Some(game)) => locate::locate_steam_game(&game),
        (None, None) => bail!("no game specified"),
    }
}
