use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use byteorder::{LE, WriteBytesExt};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::GameFiles;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::unity::types::MonoBehaviour;

fn main() -> Result<()> {
    let tpk = TpkTypeTreeBlob::embedded();
    let tpk = TypeTreeCache::new(tpk);

    let mut args = std::env::args().skip(1);
    let game_dir = args
        .next()
        .context("expected path to game dir as first argument")?;
    let game_dir = Path::new(&game_dir);
    let out_path = args
        .next()
        .context("expected output file as second argument")?;

    let game_files = GameFiles::probe(game_dir)?;
    let env = Environment::new(game_files, tpk);

    let backend: GeneratorBackend = GeneratorBackend::default();
    let generator = TypeTreeGenerator::new_lib_next_to_exe(env.unity_version()?, backend)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;

    let used = collect_used_script_types(env)?;
    let mb_typetrees = generate_monobehaviour_types(used, generator)?;

    /*
     * n_assemblies x
     *  len name
     *  n_types x:
     *    len full_name
     *    n_flat_nodes x:
     *      len name
     *      len type
     *      u8  index
     *      i32 flags
     */
    let mut export = Vec::new();
    export.write_u32::<LE>(mb_typetrees.len() as u32)?;
    for (assembly, types) in &mb_typetrees {
        export.write_u32::<LE>(assembly.len() as u32)?;
        export.write_all(assembly.as_bytes())?;

        export.write_u32::<LE>(types.len() as u32)?;
        for (ty_name, ty_data) in types {
            export.write_u32::<LE>(ty_name.len() as u32)?;
            export.write_all(ty_name.as_bytes())?;

            export.write_u32::<LE>(ty_data.len() as u32)?;
            for &(ref name, ref ty, level, flags) in ty_data {
                export.write_u32::<LE>(name.len() as u32)?;
                export.write_all(name.as_bytes())?;
                export.write_u32::<LE>(ty.len() as u32)?;
                export.write_all(ty.as_bytes())?;
                export.write_u8(level)?;
                export.write_i32::<LE>(flags)?;
            }
        }
    }

    let export = lz4_flex::compress_prepend_size(&export);
    std::fs::write(out_path, export)?;

    Ok(())
}

fn generate_monobehaviour_types(
    used: BTreeMap<String, BTreeSet<String>>,
    generator: TypeTreeGenerator,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<(String, String, u8, i32)>>>, anyhow::Error> {
    let definitions = generator.get_monobehaviour_definitions()?;

    let mut output: BTreeMap<String, BTreeMap<String, Vec<_>>> = BTreeMap::default();
    for (assembly, used_paths) in used {
        let Some(definitions_classes) = definitions.get(&assembly) else {
            println!("{assembly} not loaded");
            continue;
        };
        println!("Assembly: {}", assembly);

        let mut x = BTreeMap::default();
        for path in used_paths {
            if !definitions_classes.contains(&path) {
                eprintln!("{path} not found in {assembly}");
                continue;
            }

            println!("  Path: {}", path);
            let generate_typetree_json = generator.generate_typetree_json(&assembly, &path)?;
            let json = serde_json::from_str::<Vec<TypetreeNodeDump>>(&generate_typetree_json)?;

            x.insert(
                path,
                json.into_iter()
                    .map(|x| (x.m_Type, x.m_Name, x.m_Level, x.m_MetaFlag))
                    .collect(),
            );
        }
        output.insert(assembly, x);
    }
    Ok(output)
}

fn collect_used_script_types(env: Environment) -> Result<BTreeMap<String, BTreeSet<String>>> {
    env.resolver
        .all_files()?
        .into_par_iter()
        .map(|file| -> Result<_> {
            let mut used = Vec::new();
            let name = file.file_name().unwrap().to_str().unwrap();
            if name.starts_with("level") {
                // PERF: this can be optimized for the BundleFileReader resolver
                let (serialized, mut data) = env.load_leaf(file)?;
                for mb in serialized.objects_of::<MonoBehaviour>(&env.tpk)? {
                    let mb = mb.read(&mut data)?;

                    if mb.m_Script.is_null() {
                        continue;
                    }

                    let script = env.deref_read(mb.m_Script, &serialized, &mut data)?;

                    used.push((
                        script.assembly_name().into_owned(),
                        script.full_name().into_owned(),
                    ));
                }
            }

            Ok(used)
        })
        .try_fold(
            BTreeMap::<String, BTreeSet<String>>::default,
            |mut acc, item| -> Result<_> {
                for (asm, full_name) in item? {
                    acc.entry(asm).or_default().insert(full_name);
                }
                Ok(acc)
            },
        )
        .try_reduce(BTreeMap::default, |mut map1, map2| {
            for (k, v_set) in map2 {
                map1.entry(k).or_default().extend(v_set);
            }
            Ok(map1)
        })
}

#[derive(serde_derive::Deserialize, Debug)]
#[allow(non_snake_case)]
struct TypetreeNodeDump {
    m_Type: String,
    m_Name: String,
    m_Level: u8,
    m_MetaFlag: i32,
}
