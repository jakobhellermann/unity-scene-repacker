mod scene_lookup;
mod trace_pptr;
mod typetree_cache;
mod unity;
mod utils;

use anyhow::{Context, Result, ensure};
use clap::{Args, CommandFactory as _, Parser};
use clap_complete::{ArgValueCompleter, CompletionCandidate};
use indexmap::IndexMap;
use memmap2::Mmap;
use paris::{error, info, success, warn};
use rabex::config::ExtractionConfig;
use rabex::files::SerializedFile;
use rabex::files::bundlefile::{self, BundleFileHeader, BundleFileReader, CompressionType};
use rabex::files::serialzedfile::builder::SerializedFileBuilder;
use rabex::files::serialzedfile::{self, TypeTreeProvider};
use rabex::objects::pptr::{PPtr, PathId};
use rabex::objects::{ClassId, TypedPPtr};
use rabex::serde_typetree;
use rabex::tpk::{TpkFile, TpkTypeTreeBlob, UnityVersion};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use utils::TempDir;

use crate::scene_lookup::SceneLookup;
use crate::typetree_cache::TypeTreeCache;
use crate::unity::types::{
    AssetBundle, AssetInfo, BuildSettings, GameObject, PreloadData, Transform,
};
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

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let start = Instant::now();

    let preloads = std::fs::read_to_string(&args.objects)
        .with_context(|| format!("couldn't find object json '{}'", args.objects.display()))?;
    let preloads: IndexMap<String, Vec<String>> = json5::from_str(&preloads)?;

    let obj_count = preloads
        .iter()
        .map(|(_, objects)| objects.len())
        .sum::<usize>();
    info!("Repacking {obj_count} objects in {} scenes", preloads.len());

    let tpk_file = TpkFile::from_reader(&mut include_bytes!("../resources/lz4.tpk").as_slice())?;
    let tpk = tpk_file.as_type_tree()?.unwrap();
    let tt = TypeTreeCache::new(&tpk);

    let temp_dir = TempDir::named_in_tmp("unity-scene-repacker")?;

    let mut repack_scenes = Vec::new();
    let bundle = game_dir.join("data.unity3d");
    if bundle.exists() {
        let mut reader = BundleFileReader::from_reader(
            BufReader::new(File::open(bundle)?),
            &ExtractionConfig::default(),
        )?;

        let mut scenes = None;
        while let Some(mut item) = reader.next() {
            if item.path == "globalgamemanagers" {
                let mut ggm_reader = Cursor::new(item.read()?);
                let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

                let build_settings =
                    ggm.read_single::<BuildSettings>(ClassId::BuildSettings, &tt, &mut ggm_reader)?;
                scenes = Some(
                    build_settings
                        .scene_names()
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>(),
                );
            } else if let Some(index) = item.path.strip_prefix("level") {
                if let Ok(val) = index.parse::<usize>() {
                    let scenes = scenes.as_ref().context(
                        "globalgamemanagers not found in data.unity3d before level files",
                    )?;
                    let scene_name = &scenes[val];
                    let Some(paths) = preloads.get(scene_name.as_str()) else {
                        continue;
                    };

                    let data = item.read()?;

                    let (serialized, all_reachable, roots) =
                        prune_scene(&scene_name, Cursor::new(data), &tt, &paths)?;

                    let tmp = temp_dir.dir.join(scene_name);
                    std::fs::write(&tmp, data).context("Writing bundle data to temporary file")?;

                    repack_scenes.push((scene_name.clone(), serialized, tmp, all_reachable, roots));
                }
            }
        }
    } else {
        let mut ggm_reader = File::open(game_dir.join("globalgamemanagers"))
            .context("couldn't find globalgamemanagers in game directory")?;
        let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

        let scenes =
            ggm.read_single::<BuildSettings>(ClassId::BuildSettings, &tt, &mut ggm_reader)?;
        let scenes: FxHashMap<_, _> = scenes
            .scene_names()
            .enumerate()
            .map(|(i, path)| (path, i))
            .collect();

        for (scene_name, paths) in preloads {
            let scene_index = scenes[scene_name.as_str()];
            let path = game_dir.join(format!("level{scene_index}"));

            let file = File::open(&path)?;
            let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

            let (serialized, all_reachable, roots) =
                prune_scene(&scene_name, &mut data, &tt, &paths)?;
            repack_scenes.push((scene_name, serialized, path, all_reachable, roots));
        }
    }

    let unity_version: UnityVersion = "2020.2.2f1".parse().unwrap();

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

    repack_bundle(
        name,
        &mut out,
        compression,
        &tpk,
        &tt,
        unity_version,
        args.disable,
        repack_scenes.as_mut_slice(),
    )
    .context("trying to repack bundle")?;

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
    mut data: impl Read + Seek,
    typetree_provider: impl TypeTreeProvider,
    retain_paths: &[String],
) -> Result<(SerializedFile, BTreeSet<PathId>, Vec<PathId>)> {
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

    Ok((serialized, all_reachable, new_roots.into_iter().collect()))
}

fn adjust_roots(
    replacements: &mut FxHashMap<i64, Vec<u8>>,
    tpk: &TpkTypeTreeBlob,
    serialized: &mut SerializedFile,
    data: &mut Cursor<Mmap>,
    transform: i64,
    disable: bool,
) -> Result<(), anyhow::Error> {
    let transform_info = serialized.get_object(transform).unwrap();
    let tt = serialized.get_typetree_for(&transform_info, tpk)?;
    let mut transform = serialized.read_as::<Transform>(transform_info, &tt, data)?;
    transform.m_Father = TypedPPtr::null();

    let transform_modified =
        serde_typetree::to_vec_endianed(&transform, &tt, serialized.m_Header.m_Endianess)?;
    replacements.insert(transform_info.m_PathID, transform_modified);

    if disable {
        let go_info = transform.m_GameObject.deref_local(serialized);
        let tt = serialized.get_typetree_for(&go_info, tpk)?;
        let mut go = serialized.read_as::<GameObject>(&go_info, &tt, data)?;
        go.m_IsActive = false;
        let go_modified =
            serde_typetree::to_vec_endianed(&go, &tt, serialized.m_Header.m_Endianess)?;
        replacements.insert(go_info.m_PathID, go_modified);
    }

    Ok(())
}

#[must_use]
fn prune_types(serialized: &mut SerializedFile) -> FxHashMap<i32, i32> {
    let used_types: FxHashSet<_> = serialized.objects().map(|obj| obj.m_TypeID).collect();
    let mut old_to_new: FxHashMap<i32, i32> = FxHashMap::default();
    serialized.m_Types = std::mem::take(&mut serialized.m_Types)
        .into_iter()
        .enumerate()
        .filter(|&(idx, _)| used_types.contains(&(idx as i32)))
        .enumerate()
        .map(|(new_index, (old_index, ty))| {
            old_to_new.insert(old_index as i32, new_index as i32);
            ty
        })
        .collect();
    old_to_new
}

#[derive(Debug)]
struct Stats {
    objects_before: usize,
    objects_after: usize,
    size_before: usize,
    size_after: usize,
}

fn repack_bundle<W: Write + Seek>(
    bundle_name: &str,
    writer: W,
    compression: CompressionType,
    tpk: &TpkTypeTreeBlob,
    typetree_provider: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    disable_roots: bool,
    scenes: &mut [(
        String,
        SerializedFile,
        PathBuf,
        BTreeSet<PathId>,
        Vec<PathId>,
    )],
) -> Result<Stats> {
    let mut files = Vec::new();

    let mut stats = Stats {
        objects_before: 0,
        objects_after: 0,
        size_before: 0,
        size_after: 0,
    };

    let common_offset_map = serialzedfile::build_common_offset_map(tpk, unity_version);

    let prefix = bundle_name;

    let container = scenes
        .iter()
        .map(|(scene_name, ..)| {
            let path = format!("unity-scene-repacker/{prefix}_{scene_name}.unity");
            (path, AssetInfo::default())
        })
        .collect();
    let mut container = Some(container);

    for (name, serialized, path, keep_objects, roots) in scenes {
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
                m_Name: bundle_name.to_owned(),
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
            let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

            stats.objects_before += serialized.objects().len();
            stats.size_before += data.get_ref().len();

            serialized.modify_objects(|objects| {
                objects.retain(|obj| keep_objects.contains(&obj.m_PathID));
            });
            stats.objects_after += serialized.objects().len();

            let type_remap = prune_types(serialized);

            let mut replacements = FxHashMap::default();
            for &root in roots.iter() {
                adjust_roots(
                    &mut replacements,
                    tpk,
                    serialized,
                    &mut data,
                    root,
                    disable_roots,
                )?;
            }

            let new_objects = serialized.take_objects();
            let objects = new_objects.into_iter().map(|mut obj| {
                let data = match replacements.remove(&obj.m_PathID) {
                    Some(owned) => Cow::Owned(owned),
                    None => {
                        let offset = obj.m_Offset as usize;
                        let size = obj.m_Size as usize;
                        Cow::Borrowed(&data.get_ref()[offset..offset + size])
                    }
                };

                obj.m_TypeID = type_remap[&obj.m_TypeID];

                (obj, data)
            });

            let mut writer = Cursor::new(Vec::new());
            serialzedfile::write_serialized_with(
                &mut writer,
                serialized,
                &common_offset_map,
                objects,
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
        writer,
        CompressionType::Lz4hc,
        compression,
        files.into_iter(),
    )?;

    Ok(stats)
}
