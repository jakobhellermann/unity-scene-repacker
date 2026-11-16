#![allow(clippy::missing_safety_doc)]
use std::ffi::{CStr, CString, c_char, c_int};
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use indexmap::IndexMap;
use rabex::objects::ClassId;
use rabex::typetree::TypeTreeProvider as _;
use rabex_env::Environment;
use rabex_env::resolver::GameFiles;
use rabex_env::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::rabex::files::bundlefile::CompressionType;
use unity_scene_repacker::rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::{
    MonobehaviourTypetreeMode, RepackSettings, Stats, dependency_addressables,
    monobehaviour_typetree_export,
};

#[repr(C)]
pub struct CStats {
    pub objects_before: c_int,
    pub objects_after: c_int,
}

enum Mode {
    SceneBundle = 0,
    AssetBundle = 1,
    AssetBundleShallow = 2,
}
impl Mode {
    fn needs_typetree_generator(&self) -> bool {
        matches!(self, Mode::AssetBundle)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn export(
    name: *const c_char,
    game_dir: *const c_char,
    preload_json: *const c_char,
    error: *mut *const c_char,
    bundle_size: *mut c_int,
    bundle_data: *mut *mut u8,
    stats_ret: *mut CStats,
    mb_typetree_export: *const u8,
    mb_typetree_len: c_int,
    mode: u8,
) {
    unsafe {
        let name = CStr::from_ptr(name);
        let game_dir = CStr::from_ptr(game_dir);
        let preload_json = CStr::from_ptr(preload_json);

        let mb_typetree_export = (!mb_typetree_export.is_null())
            .then(|| std::slice::from_raw_parts(mb_typetree_export, mb_typetree_len as usize));

        let result = export_inner(name, game_dir, preload_json, mode, mb_typetree_export);
        match result {
            Ok((stats, data)) => {
                *bundle_size = data.len() as c_int;
                *bundle_data = Box::into_raw(data.into_boxed_slice()).cast();
                *stats_ret = CStats {
                    objects_before: stats.objects_before as c_int,
                    objects_after: stats.objects_after as c_int,
                }
            }
            Err(e) => {
                let error_string = CString::new(e.to_string()).unwrap_or_else(|_| c"".to_owned());
                *error = CString::into_raw(error_string);
                *bundle_size = 0;
                *bundle_data = std::ptr::null_mut();
            }
        }
    }
}

/// A game the dependencies of repacked catalogs are bundled from.
pub struct Game {
    env: Environment,
    prefix: String,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_open(
    data_dir: *const c_char,
    error: *mut *const c_char,
) -> *mut Game {
    match game_open_inner(unsafe { CStr::from_ptr(data_dir) }) {
        Ok(game) => Box::into_raw(Box::new(game)),
        Err(e) => {
            unsafe { *error = error_string(e) };
            std::ptr::null_mut()
        }
    }
}

fn game_open_inner(data_dir: &CStr) -> Result<Game> {
    let game_files = GameFiles::probe(Path::new(data_dir.to_str()?))?;
    let mut env = Environment::new(game_files, TypeTreeCache::new(TpkTypeTreeBlob::embedded()));
    let unity_version = env.unity_version()?.clone();
    let monobehaviour_node = env
        .tpk
        .get_typetree_node(ClassId::MonoBehaviour, &unity_version)
        .context("no MonoBehaviour type tree for this unity version")?
        .into_owned();
    env.typetree_generator = TypeTreeGeneratorCache::new(unity_version, monobehaviour_node);
    let prefix = dependency_addressables::game_prefix(&env)?;
    Ok(Game { env, prefix })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_free(game: *mut Game) {
    drop(unsafe { Box::from_raw(game) });
}

/// Builds the bundle of the game file `file`, to be freed with `free_array`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_file_bundle(
    game: *const Game,
    file: *const c_char,
    error: *mut *const c_char,
    bundle_size: *mut c_int,
    bundle_data: *mut *mut u8,
) {
    unsafe {
        let game = &*game;
        let result = CStr::from_ptr(file)
            .to_str()
            .map_err(anyhow::Error::from)
            .and_then(|file| {
                dependency_addressables::bundle_game_file(&game.env, &game.prefix, file)
            });
        match result {
            Ok(data) => {
                *bundle_size = data.len() as c_int;
                *bundle_data = Box::into_raw(data.into_boxed_slice()).cast();
            }
            Err(e) => {
                *error = error_string(e);
                *bundle_size = 0;
                *bundle_data = std::ptr::null_mut();
            }
        }
    }
}

fn error_string(e: anyhow::Error) -> *const c_char {
    CString::into_raw(CString::new(format!("{e:?}")).unwrap_or_else(|_| c"".to_owned()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free_str(cstr: *mut c_char) {
    unsafe { drop(CString::from_raw(cstr)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn free_array(len: c_int, data: *mut u8) {
    unsafe {
        let data = std::ptr::slice_from_raw_parts_mut(data, len as usize);
        drop(Box::from_raw(data))
    };
}

fn export_inner(
    name: &CStr,
    game_dir: &CStr,
    scene_objects_json: &CStr,
    mode: u8,
    mb_typetree_export: Option<&[u8]>,
) -> Result<(Stats, Vec<u8>)> {
    let name = name.to_str()?;
    let game_dir = Path::new(game_dir.to_str()?);
    let scene_objects = scene_objects_json.to_str()?;
    let mode = match mode {
        0 => Mode::SceneBundle,
        1 => Mode::AssetBundle,
        2 => Mode::AssetBundleShallow,
        _ => bail!("Expected 0=SceneBundle, 1=AssetBundle or 2=AssetBundleShallow, got {mode}"),
    };

    let tpk_raw = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let compression = CompressionType::None;

    let scene_objects: IndexMap<String, Vec<String>> =
        serde_json::from_str(scene_objects).context("error parsing the objects json")?;

    let repack_settings = RepackSettings {
        scene_objects,
        extra_objects: IndexMap::new(),
    };

    let disable = true;

    let mut out = Cursor::new(Vec::new());

    let game_files = GameFiles::probe(game_dir)?;
    let mut env = Environment::new(game_files, tpk);
    let unity_version = env.unity_version()?.clone();

    if mode.needs_typetree_generator() {
        let monobehaviour_typetree_mode = match mb_typetree_export {
            Some(data) => MonobehaviourTypetreeMode::Export(data),
            None => MonobehaviourTypetreeMode::GenerateRuntime,
        };

        let monobehaviour_node = env
            .tpk
            .get_typetree_node(ClassId::MonoBehaviour, &unity_version)
            .unwrap()
            .into_owned();

        env.typetree_generator = match monobehaviour_typetree_mode {
            MonobehaviourTypetreeMode::GenerateRuntime => {
                TypeTreeGeneratorCache::new(env.unity_version()?.clone(), monobehaviour_node)
            }
            MonobehaviourTypetreeMode::Export(export) => {
                TypeTreeGeneratorCache::prefilled(monobehaviour_typetree_export::read(export)?)
            }
        };
    }

    if let Mode::AssetBundleShallow = mode {
        let stats = unity_scene_repacker::pack_to_shallow_asset_bundle(
            &env,
            &mut out,
            name,
            repack_settings,
            compression,
        )?;
        return Ok((stats, out.into_inner()));
    }

    let (mut repack_scenes, extra_objects) = unity_scene_repacker::repack_scenes(
        &env,
        repack_settings,
        disable,
        matches!(mode, Mode::AssetBundle),
    )?;

    let enable_typetree = false; // TODO: make this configurable / infer if necessary

    let stats = match mode {
        Mode::SceneBundle => unity_scene_repacker::pack_to_scene_bundle(
            &mut out,
            name,
            &tpk_raw,
            &env.tpk,
            &unity_version,
            repack_scenes.as_mut_slice(),
            compression,
        )
        .context("trying to repack bundle")?,
        Mode::AssetBundle => unity_scene_repacker::pack_to_asset_bundle(
            &env,
            &mut out,
            name,
            &tpk_raw,
            repack_scenes,
            extra_objects,
            compression,
            enable_typetree,
            None,
        )?,
        Mode::AssetBundleShallow => unreachable!(),
    };

    Ok((stats, out.into_inner()))
}
