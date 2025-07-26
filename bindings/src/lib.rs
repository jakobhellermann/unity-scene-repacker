#![allow(clippy::missing_safety_doc)]
use std::ffi::{CStr, CString, c_char, c_int};
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use indexmap::IndexMap;
use rabex::objects::ClassId;
use rabex::typetree::TypeTreeProvider as _;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::rabex::files::bundlefile::CompressionType;
use unity_scene_repacker::rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::{
    GameFiles, MonobehaviourTypetreeMode, RepackSettings, Stats, monobehaviour_typetree_export,
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

    let repack_settings = RepackSettings { scene_objects };

    let disable = true;

    let mut out = Cursor::new(Vec::new());

    let game_files = GameFiles::probe(game_dir)?;
    let mut env = Environment::new(game_files, tpk);
    let unity_version = env.unity_version()?;

    if mode.needs_typetree_generator() {
        let monobehaviour_typetree_mode = match mb_typetree_export {
            Some(data) => MonobehaviourTypetreeMode::Export(data),
            None => MonobehaviourTypetreeMode::GenerateRuntime,
        };

        let monobehaviour_node = env
            .tpk
            .get_typetree_node(ClassId::MonoBehaviour, unity_version)
            .unwrap()
            .into_owned();

        env.typetree_generator = match monobehaviour_typetree_mode {
            MonobehaviourTypetreeMode::GenerateRuntime => {
                let generator = TypeTreeGenerator::new_lib_next_to_exe(
                    unity_version,
                    GeneratorBackend::AssetsTools,
                )?;
                generator
                    .load_all_dll_in_dir(game_dir.join("Managed"))
                    .context("Cannot load game DLLs")?;
                TypeTreeGeneratorCache::new(generator, monobehaviour_node)
            }
            MonobehaviourTypetreeMode::Export(export) => TypeTreeGeneratorCache::prefilled(
                monobehaviour_typetree_export::read(export)?,
                monobehaviour_node,
            ),
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

    let mut repack_scenes = unity_scene_repacker::repack_scenes(
        &env,
        repack_settings,
        disable,
        matches!(mode, Mode::AssetBundle),
    )?;

    let stats = match mode {
        Mode::SceneBundle => unity_scene_repacker::pack_to_scene_bundle(
            &mut out,
            name,
            &tpk_raw,
            &env.tpk,
            unity_version,
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
            compression,
        )?,
        Mode::AssetBundleShallow => unreachable!(),
    };

    Ok((stats, out.into_inner()))
}
