#![allow(clippy::missing_safety_doc)]
use std::ffi::{CStr, CString, c_char, c_int};
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};
use indexmap::IndexMap;
use rabex_env::Environment;
use rabex_env::resolver::GameFiles;
use unity_scene_repacker::dependency_addressables::Dependencies;
use unity_scene_repacker::rabex::files::bundlefile::CompressionType;
use unity_scene_repacker::rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::rewrite_rules::RewriteRule;
use unity_scene_repacker::{RepackSettings, Stats, dependency_addressables};

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

/// A game to repack objects from.
pub struct Game {
    env: Environment,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_open(
    game_dir: *const c_char,
    error: *mut *const c_char,
) -> *mut Game {
    let result = (|| -> Result<Game> {
        let game_dir = unsafe { CStr::from_ptr(game_dir) }.to_str()?;
        let game_files = GameFiles::probe(Path::new(game_dir))?;
        let env = Environment::new(game_files, TypeTreeCache::new(TpkTypeTreeBlob::embedded()));
        Ok(Game { env })
    })();
    match result {
        Ok(game) => Box::into_raw(Box::new(game)),
        Err(e) => {
            unsafe { *error = error_string(e) };
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn game_free(game: *mut Game) {
    drop(unsafe { Box::from_raw(game) });
}

/// Options of [`repack`]. Flags that don't apply to `mode` must be false.
#[repr(C)]
pub struct RepackOptions {
    /// 0 = scene bundle, 1 = asset bundle, 2 = shallow asset bundle
    pub mode: u8,
    /// 0 = none, 2 = lz4
    pub compression: u8,
    /// scene and asset bundle
    pub disable_roots: bool,
    /// asset bundle
    pub write_typetrees: bool,
    /// asset bundle: reference the game files the bundle depends on as `deps/<file>.bundle`
    /// (see `dependency_file_bundle`), and return an Addressables catalog for it
    pub emit_catalog: bool,
    /// asset bundle: see [`rewrite_rules`]
    pub rewrite_rules: *const c_char,
}

/// Repacks the objects of `objects_json` (JSON5, scene name to gameobject paths) into a bundle.
///
/// With `emit_catalog`, `catalog_data` receives the catalog and `dependencies` the
/// newline-separated game files the catalog references, otherwise they are set to null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn repack(
    game: *const Game,
    name: *const c_char,
    objects_json: *const c_char,
    options: *const RepackOptions,
    error: *mut *const c_char,
    stats: *mut CStats,
    bundle_size: *mut c_int,
    bundle_data: *mut *mut u8,
    catalog_size: *mut c_int,
    catalog_data: *mut *mut u8,
    dependencies: *mut *const c_char,
) {
    unsafe {
        let game = &*game;
        let options = &*options;
        let result = (|| -> Result<_> {
            let name = CStr::from_ptr(name).to_str()?;
            let objects_json = CStr::from_ptr(objects_json).to_str()?;
            let rewrite_rules = rewrite_rules(options.rewrite_rules)?;
            repack_inner(&game.env, name, objects_json, options, &rewrite_rules)
        })();
        *catalog_size = 0;
        *catalog_data = std::ptr::null_mut();
        *dependencies = std::ptr::null();
        match result {
            Ok(result) => {
                *stats = CStats {
                    objects_before: result.stats.objects_before as c_int,
                    objects_after: result.stats.objects_after as c_int,
                };
                write_array(result.bundle, bundle_size, bundle_data);
                if let Some((catalog, files)) = result.catalog {
                    write_array(catalog, catalog_size, catalog_data);
                    *dependencies = CString::new(files.join("\n"))
                        .expect("game file paths contain no nul bytes")
                        .into_raw();
                }
            }
            Err(e) => {
                *error = error_string(e);
                write_null(bundle_size, bundle_data);
            }
        }
    }
}

struct RepackResult {
    stats: Stats,
    bundle: Vec<u8>,
    /// The catalog and the game files it references
    catalog: Option<(Vec<u8>, Vec<String>)>,
}

fn repack_inner(
    env: &Environment,
    name: &str,
    objects_json: &str,
    options: &RepackOptions,
    rewrite_rules: &[RewriteRule],
) -> Result<RepackResult> {
    let mode = match options.mode {
        0 => Mode::SceneBundle,
        1 => Mode::AssetBundle,
        2 => Mode::AssetBundleShallow,
        mode => bail!("Expected 0=SceneBundle, 1=AssetBundle or 2=AssetBundleShallow, got {mode}"),
    };
    let compression = compression_type(options.compression)?;
    if !matches!(mode, Mode::AssetBundle) {
        ensure!(
            !options.write_typetrees,
            "write_typetrees is only supported for asset bundles"
        );
        ensure!(
            !options.emit_catalog,
            "emit_catalog is only supported for asset bundles"
        );
        ensure!(
            rewrite_rules.is_empty(),
            "rewrite_rules are only supported for asset bundles"
        );
    }
    if matches!(mode, Mode::AssetBundleShallow) {
        ensure!(
            !options.disable_roots,
            "disable_roots is not supported for shallow asset bundles"
        );
    }

    let scene_objects: IndexMap<String, Vec<String>> =
        json5::from_str(objects_json).context("error parsing the objects json")?;
    let repack_settings = RepackSettings {
        scene_objects,
        extra_objects: IndexMap::new(),
        strict: true,
    };

    let mut bundle = Cursor::new(Vec::new());
    let tpk_blob = TpkTypeTreeBlob::embedded();

    if let Mode::AssetBundleShallow = mode {
        let stats = unity_scene_repacker::pack_to_shallow_asset_bundle(
            env,
            &mut bundle,
            name,
            repack_settings,
            compression,
        )?;
        return Ok(RepackResult {
            stats,
            bundle: bundle.into_inner(),
            catalog: None,
        });
    }

    let is_asset_bundle = matches!(mode, Mode::AssetBundle);
    let (mut repack_scenes, extra_objects) = unity_scene_repacker::repack_scenes(
        env,
        repack_settings,
        is_asset_bundle,
        options.disable_roots,
    )?;

    if !is_asset_bundle {
        let stats = unity_scene_repacker::pack_to_scene_bundle(
            &mut bundle,
            name,
            &tpk_blob,
            &env.tpk,
            env.unity_version()?,
            repack_scenes.as_mut_slice(),
            compression,
        )?;
        return Ok(RepackResult {
            stats,
            bundle: bundle.into_inner(),
            catalog: None,
        });
    }

    let dependencies = options
        .emit_catalog
        .then(|| Dependencies::new(env, &repack_scenes))
        .transpose()?;
    let stats = unity_scene_repacker::pack_to_asset_bundle(
        env,
        &mut bundle,
        name,
        &tpk_blob,
        repack_scenes,
        extra_objects,
        compression,
        options.write_typetrees || options.emit_catalog,
        dependencies.as_ref(),
        rewrite_rules,
    )?;

    let catalog = dependencies
        .map(|dependencies| -> Result<_> {
            let catalog = dependencies.catalog(env, &format!("{name}.bundle"), name)?;
            let files = dependencies
                .files()
                .into_iter()
                .map(str::to_owned)
                .collect();
            Ok((catalog.to_binary()?, files))
        })
        .transpose()?;

    Ok(RepackResult {
        stats,
        bundle: bundle.into_inner(),
        catalog,
    })
}

/// Builds the bundle `deps/<file>.bundle` referenced by repacked bundles and catalogs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dependency_file_bundle(
    game: *const Game,
    file: *const c_char,
    compression: u8,
    rewrite_rules: *const c_char,
    error: *mut *const c_char,
    bundle_size: *mut c_int,
    bundle_data: *mut *mut u8,
) {
    unsafe {
        let game = &*game;
        let result = (|| -> Result<_> {
            let file = CStr::from_ptr(file).to_str()?;
            let compression = compression_type(compression)?;
            let rewrite_rules = self::rewrite_rules(rewrite_rules)?;
            dependency_addressables::bundle_game_file(&game.env, file, compression, &rewrite_rules)
                .with_context(|| format!("bundling {file}"))
        })();
        match result {
            Ok(bundle) => write_array(bundle, bundle_size, bundle_data),
            Err(e) => {
                *error = error_string(e);
                write_null(bundle_size, bundle_data);
            }
        }
    }
}

/// JSON5 `[{ "assembly": "Assembly-CSharp", "script": "Namespace.Class", "field": "a.b", "value": 0 }]`, written into
/// the MonoBehaviours of those scripts; null for none.
unsafe fn rewrite_rules(json: *const c_char) -> Result<Vec<RewriteRule>> {
    if json.is_null() {
        return Ok(Vec::new());
    }
    let json = unsafe { CStr::from_ptr(json) }.to_str()?;
    json5::from_str(json).context("error parsing the rewrite rules")
}

/// Only the compressions rabex is built with here, using the same numbering as `CompressionType`
fn compression_type(compression: u8) -> Result<CompressionType> {
    Ok(match compression {
        0 => CompressionType::None,
        2 => CompressionType::Lz4,
        _ => bail!("Expected compression 0=None or 2=Lz4, got {compression}"),
    })
}

/// Hands `data` to the caller, to be freed with `free_array`.
unsafe fn write_array(data: Vec<u8>, size: *mut c_int, ptr: *mut *mut u8) {
    unsafe {
        *size = data.len() as c_int;
        *ptr = Box::into_raw(data.into_boxed_slice()).cast();
    }
}

unsafe fn write_null(size: *mut c_int, ptr: *mut *mut u8) {
    unsafe {
        *size = 0;
        *ptr = std::ptr::null_mut();
    }
}

fn error_string(e: anyhow::Error) -> *const c_char {
    let message = format!("{e:?}").replace('\0', "\\0");
    CString::into_raw(CString::new(message).expect("nul bytes were escaped"))
}

#[unsafe(no_mangle)]
pub extern "C" fn format_version() -> u32 {
    unity_scene_repacker::FORMAT_VERSION
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

const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<Game>()
};
