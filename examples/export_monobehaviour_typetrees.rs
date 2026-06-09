use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use byteorder::{LE, WriteBytesExt};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rabex::typetree::{TypeTreeNode, TypeTreeProvider};
use rabex_env::Environment;
use rabex_env::handle::SerializedFileHandle;
use rabex_env::resolver::EnvResolver;
use rabex_env::typetree_generator_cache::AssemblyTypeTreeGenerator;
use rabex_env::unity::types::MonoBehaviour;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use unity_scene_repacker::GameFiles;

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
        .context("expected path to output file as second argument")?;

    let game_files = GameFiles::probe(game_dir)?;
    let env = Environment::new(game_files, tpk);
    let generator = env.typetree_generator.backend(&env)?;

    let managed_dir = env.game_files.game_dir.join("Managed");
    for entry in std::fs::read_dir(&managed_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|x| x == "dll") {
            generator.load_assembly(entry.file_name().to_str().expect("non-utf8 dll name"))?;
        }
    }

    let used = collect_used_script_types(&env).context("Could not collect used script types")?;
    let mb_typetrees =
        generate_monobehaviour_types(used, &generator).context("Could not generate typetrees")?;

    /*
     * n_assemblies x
     *  len name
     *  n_types x:
     *    len full_name
     *    n_flat_nodes x:
     *      len name
     *      len type
     *      u8  level
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

fn generate_monobehaviour_types<R: EnvResolver, P: TypeTreeProvider>(
    used: BTreeMap<String, BTreeSet<String>>,
    generator: &AssemblyTypeTreeGenerator<'_, R, P>,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<(String, String, u8, i32)>>>> {
    let definitions = generator.monobehaviour_definitions()?;

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
            let Some(tree) = generator.generate(&assembly, &path)? else {
                eprintln!("{path} in {assembly}: generate returned None");
                continue;
            };
            x.insert(path, flatten_typetree(&tree));
        }
        output.insert(assembly, x);
    }
    Ok(output)
}

fn flatten_typetree(root: &TypeTreeNode) -> Vec<(String, String, u8, i32)> {
    fn walk(node: &TypeTreeNode, level: u8, out: &mut Vec<(String, String, u8, i32)>) {
        out.push((
            node.m_Type.clone(),
            node.m_Name.clone(),
            level,
            node.m_MetaFlag.unwrap_or(0),
        ));
        for child in &node.children {
            walk(child, level + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out
}

fn collect_used_script_types(env: &Environment) -> Result<BTreeMap<String, BTreeSet<String>>> {
    env.game_files
        .all_files()?
        .into_par_iter()
        .map(|file| -> Result<_> {
            let mut used = Vec::new();
            let name = file.file_name().unwrap().to_str().unwrap();
            if name.starts_with("level") {
                let (file, data) = env.load_serialized_uncached(file)?;
                let file = SerializedFileHandle::new(&env, &file, data.as_ref());

                for mb in file.objects_of::<MonoBehaviour>() {
                    let Some(script) = mb.mono_script()? else {
                        continue;
                    };

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
