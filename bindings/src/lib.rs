use std::ffi::{CStr, CString, c_char, c_int};
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use indexmap::IndexMap;
use unity_scene_repacker::rabex::UnityVersion;
use unity_scene_repacker::rabex::files::bundlefile::{self, CompressionType};
use unity_scene_repacker::rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::{GameFiles, MonobehaviourTypetreeMode, Stats};

#[repr(C)]
pub struct CStats {
    pub objects_before: c_int,
    pub objects_after: c_int,
}

enum Mode {
    SceneBundle = 0,
    AssetBundle = 1,
}

#[unsafe(no_mangle)]
pub extern "C" fn export(
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

#[unsafe(no_mangle)]
pub extern "C" fn free_str(cstr: *mut c_char) {
    unsafe { drop(CString::from_raw(cstr)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn free_array(len: c_int, data: *mut u8) {
    unsafe {
        let data = std::slice::from_raw_parts_mut(data, len as usize) as *mut [u8];
        drop(Box::from_raw(data))
    };
}

fn export_inner(
    name: &CStr,
    game_dir: &CStr,
    preload_json: &CStr,
    mode: u8,
    mb_typetree_export: Option<&[u8]>,
) -> Result<(Stats, Vec<u8>)> {
    let name = name.to_str()?;
    let game_dir = Path::new(game_dir.to_str()?);
    let preload_json = preload_json.to_str()?;
    let mode = match mode {
        0 => Mode::SceneBundle,
        1 => Mode::AssetBundle,
        _ => bail!("Expected 0=SceneBundle or 1=AssetBundle, got {mode}"),
    };

    let tpk_raw = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());

    let compression = CompressionType::None;

    let preloads: IndexMap<String, Vec<String>> =
        serde_json::from_str(&preload_json).context("error parsing the objects json")?;

    let temp_dir = Path::new("/tmp/todo"); // unused for hollowknight
    let disable = true;

    let unity_version: UnityVersion = "2020.2.2f1".parse().unwrap();

    let mut game_files = GameFiles::probe(game_dir)?;
    let mut repack_scenes =
        unity_scene_repacker::repack_scenes(&mut game_files, preloads, &tpk, temp_dir, disable)?;

    let mut out = Cursor::new(Vec::new());

    let monobehaviour_typetree_mode = match mb_typetree_export {
        Some(data) => MonobehaviourTypetreeMode::Export(data),
        None => MonobehaviourTypetreeMode::GenerateRuntime,
    };

    let stats = match mode {
        Mode::SceneBundle => {
            let (stats, header, files) = unity_scene_repacker::pack_to_scene_bundle(
                name,
                &tpk_raw,
                &tpk,
                unity_version,
                repack_scenes.as_mut_slice(),
            )
            .context("trying to repack bundle")?;

            bundlefile::write_bundle_iter(
                &header,
                &mut out,
                CompressionType::None,
                compression,
                files
                    .into_iter()
                    .map(|(name, file)| Ok((name, Cursor::new(file)))),
            )?;

            stats
        }
        Mode::AssetBundle => {
            let stats = unity_scene_repacker::pack_to_asset_bundle(
                game_files,
                &mut out,
                name,
                &tpk_raw,
                &tpk,
                monobehaviour_typetree_mode,
                unity_version,
                repack_scenes,
                compression,
            )?;
            stats
        }
    };

    Ok((stats, out.into_inner()))
}
