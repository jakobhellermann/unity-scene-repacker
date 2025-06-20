mod scene_lookup;
mod trace_pptr;
mod typetree_cache;
mod unity;
mod utils;

use anyhow::{Context, Result};
use clap::Parser;
use indexmap::IndexMap;
use memmap2::Mmap;
use paris::{error, info, success, warn};
use rabex::files::SerializedFile;
use rabex::files::bundlefile::{self, BundleFileHeader, CompressionType};
use rabex::files::serialzedfile::builder::SerializedFileBuilder;
use rabex::files::serialzedfile::{self, TypeTreeProvider};
use rabex::objects::ClassId;
use rabex::objects::pptr::PPtr;
use rabex::tpk::{TpkFile, TpkTypeTreeBlob, UnityVersion};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::scene_lookup::SceneLookup;
use crate::typetree_cache::TypeTreeCache;
use crate::unity::types::{AssetBundle, AssetInfo, BuildSettings, PreloadData};
use crate::utils::friendly_size;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Directory where the levels files are, e.g. steam/Hollow_Knight/hollow_knight_Data1
    #[arg(long)]
    game_dir: PathBuf,
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
    #[arg(long, default_value = "none")]
    compression: Compression,
    #[arg(long, short = 'o', default_value = "out.unity3d")]
    output: PathBuf,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Compression {
    None = 0,
    Lzma = 1,
    Lz4 = 2,
    /// Best compression at the cost of speed
    Lz4hc = 3,
    // Lzham = 4,
}

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    if let Err(e) = run() {
        error!("{:?}", e);
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = Args::parse();

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let start = Instant::now();

    let preloads = std::fs::read_to_string(&args.objects)
        .with_context(|| format!("couldn't find object json '{}'", args.objects.display()))?;
    let preloads: IndexMap<String, Vec<String>> = json5::from_str(&preloads)?;

    let mut tpk_file = File::open("lz4.tpk").map_err(|_| {
        anyhow::anyhow!("missing lz4.tpk file, download from https://github.com/AssetRipper/Tpk")
    })?;
    let tpk_file = TpkFile::from_reader(&mut tpk_file)?;
    let tpk = tpk_file.as_type_tree()?.unwrap();
    let typetree_provider = TypeTreeCache::new(&tpk);

    let mut ggm_reader = File::open(args.game_dir.join("globalgamemanagers"))
        .context("couldn't find globalgamemanagers in game directory")?;
    let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

    let scenes = ggm
        .read_single::<BuildSettings>(ClassId::BuildSettings, &typetree_provider, &mut ggm_reader)?
        .scenes;
    let scenes: HashMap<&str, usize> = scenes
        .iter()
        .enumerate()
        .map(|(i, scene_path)| {
            let path = Path::new(scene_path).file_stem().unwrap().to_str().unwrap();
            (path, i)
        })
        .collect();

    let obj_count = preloads
        .iter()
        .map(|(_, objects)| objects.len())
        .sum::<usize>();
    info!("Repacking {obj_count} objects in {} scenes", preloads.len());

    let mut repack_scenes = Vec::new();
    for (scene_name, paths) in preloads {
        let scene_index = scenes[scene_name.as_str()];
        let path = args.game_dir.join(format!("level{scene_index}"));

        let (serialized, all_reachable) =
            prune_scene(&scene_name, &path, &typetree_provider, &paths)?;
        repack_scenes.push((scene_name, serialized, path, all_reachable));
    }

    let unity_version: UnityVersion = "2020.2.2f1".parse().unwrap();

    let mut out = BufWriter::new(File::create(&args.output)?);

    let compression = match args.compression {
        Compression::None => CompressionType::None,
        Compression::Lzma => CompressionType::Lzma,
        Compression::Lz4 => CompressionType::Lz4,
        Compression::Lz4hc => CompressionType::Lz4hc,
        // Compression::Lzham => CompressionType::Lzham,
    };
    let stats = repack_bundle(
        &mut out,
        compression,
        &tpk,
        &typetree_provider,
        unity_version,
        &mut repack_scenes,
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

    success!(
        "Repacked into <b>{}</b> <i>({})</i> in {:.2?}",
        args.output.display(),
        friendly_size(out.get_ref().metadata()?.len() as usize),
        start.elapsed()
    );

    Ok(())
}

#[inline(never)]
fn prune_scene(
    scene_name: &str,
    path: &Path,
    typetree_provider: impl TypeTreeProvider,
    retain_paths: &[String],
) -> Result<(SerializedFile, BTreeSet<i64>)> {
    let file = File::open(path)?;
    let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

    let serialized = SerializedFile::from_reader(&mut data)
        .with_context(|| format!("Could not parse {scene_name}"))?;

    let scene_lookup = SceneLookup::new(&serialized, typetree_provider, &mut data);
    let new_roots: Vec<_> = retain_paths
        .iter()
        .filter_map(|path| {
            let item = scene_lookup.lookup_path_id(&mut data, path);
            if item.is_none() {
                warn!("Could not find path '{path}' in {scene_name}");
            }
            item
        })
        .collect();

    let mut all_reachable = scene_lookup
        .reachable(&new_roots, &mut data)
        .with_context(|| format!("Could not determine reachable nodes in {scene_name}"))?;

    for settings in serialized.objects_of_class_id(ClassId::RenderSettings) {
        all_reachable.insert(settings.m_PathID);
    }

    Ok((serialized, all_reachable))
}

#[derive(Debug)]
struct Stats {
    objects_before: usize,
    objects_after: usize,
    size_before: usize,
    size_after: usize,
}

fn repack_bundle<W: Write + Seek>(
    writer: W,
    compression: CompressionType,
    tpk: &TpkTypeTreeBlob,
    typetree_provider: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    scenes: &mut [(String, SerializedFile, PathBuf, BTreeSet<i64>)],
) -> Result<Stats> {
    let mut files = Vec::new();

    let mut stats = Stats {
        objects_before: 0,
        objects_after: 0,
        size_before: 0,
        size_after: 0,
    };

    let common_offset_map = serialzedfile::build_common_offset_map(tpk, unity_version);

    const ASSET_BUNDLE_NAME: &str = "assetBundle";
    let prefix = "bundle";

    let container = scenes
        .iter()
        .map(|(scene_name, ..)| {
            let path = format!("repacker/{prefix}_{scene_name}.unity");
            (path, AssetInfo::default())
        })
        .collect();
    let mut container = Some(container);

    for (name, serialized, path, keep_objects) in scenes {
        let mut sharedassets =
            SerializedFileBuilder::new(unity_version, typetree_provider, &common_offset_map);

        sharedassets.add_object(&PreloadData {
            m_Name: "".into(),
            m_Assets: vec![PPtr {
                m_FileID: 1,
                m_PathID: 10001,
            }],
            ..Default::default()
        })?;

        if let Some(container) = container.take() {
            sharedassets.add_object(&AssetBundle {
                m_Name: ASSET_BUNDLE_NAME.into(),
                m_Container: container,
                m_MainAsset: AssetInfo::default(),
                m_RuntimeCompatibility: 1,
                m_IsStreamedSceneAssetBundle: true,
                m_PathFlags: 7,
                ..Default::default()
            })?;
        }

        let mut out = Cursor::new(Vec::new());
        sharedassets.write(&mut out)?;
        out.set_position(0);

        files.push(Ok((
            format!("BuildPlayer-{prefix}_{name}.sharedAssets"),
            out,
        )));

        let trimmed = {
            let file = File::open(path)?;
            let data = Cursor::new(unsafe { Mmap::map(&file)? });

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.get_ref().len();

            serialized.modify_objects(|objects| {
                objects.retain(|obj| keep_objects.contains(&obj.m_PathID));
            });

            let mut writer = Cursor::new(Vec::new());
            serialzedfile::write_serialized(
                &mut writer,
                serialized,
                data.get_ref(),
                &common_offset_map,
            )?;
            let out = writer.into_inner();

            stats.objects_after += serialized.objects().len();
            stats.size_after += out.len();

            out
        };
        files.push(Ok((
            format!("BuildPlayer-{prefix}_{name}"),
            Cursor::new(trimmed),
        )));
    }

    let header = BundleFileHeader {
        signature: bundlefile::BundleSignature::UnityFS,
        version: 7,
        unity_version: "5.x.x".to_owned(),
        unity_revision: unity_version.to_string(),
        size: 0, // unused
    };

    bundlefile::write_bundle_iter(
        &header,
        writer,
        CompressionType::Lz4hc,
        compression,
        files.into_iter(),
    )?;

    Ok(stats)
}
