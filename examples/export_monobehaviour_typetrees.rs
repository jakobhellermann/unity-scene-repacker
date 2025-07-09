use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::{Context, Result};
use byteorder::{LE, WriteBytesExt};
use rabex::files::SerializedFile;
use rabex::objects::{ClassId, ClassIdType, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::env::{EnvResolver, Environment};

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

    let path = game_dir.join("globalgamemanagers");

    let mut data = &mut Cursor::new(std::fs::read(path)?);
    let file = SerializedFile::from_reader(&mut data)?;

    let env = Environment::new_in(game_dir, tpk);
    let used = collect_used_script_types(env)?;

    let backend: GeneratorBackend = GeneratorBackend::AssetsTools;
    let generator = TypeTreeGenerator::new(file.m_UnityVersion.unwrap(), backend)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;

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

fn collect_used_script_types(
    env: Environment<TypeTreeCache<TpkTypeTreeBlob>>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
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

use serde_derive::Deserialize;
#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
pub struct MonoScript {
    pub m_Name: String,
    pub m_ExecutionOrder: i32,
    pub m_PropertiesHash: [u8; 16],
    pub m_ClassName: String,
    pub m_Namespace: String,
    pub m_AssemblyName: String,
}
impl MonoScript {
    pub fn assembly_name(&self) -> Cow<'_, str> {
        match self.m_AssemblyName.ends_with(".dll") {
            true => Cow::Borrowed(&self.m_AssemblyName),
            false => Cow::Owned(format!("{}.dll", self.m_AssemblyName)),
        }
    }
    pub fn full_name(&self) -> Cow<'_, str> {
        match self.m_Namespace.is_empty() {
            true => Cow::Borrowed(&self.m_ClassName),
            false => Cow::Owned(format!("{}.{}", self.m_Namespace, self.m_ClassName)),
        }
    }
}

impl ClassIdType for MonoScript {
    const CLASS_ID: ClassId = ClassId::MonoScript;
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct MonoBehaviour {
    pub m_GameObject: TypedPPtr<GameObject>,
    pub m_Enabled: u8,
    pub m_Script: TypedPPtr<MonoScript>,
    pub m_Name: String,
}

impl ClassIdType for MonoBehaviour {
    const CLASS_ID: ClassId = ClassId::MonoBehaviour;
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct GameObject {
    // pub m_Component: Vec<ComponentPair>,
    pub m_Layer: u32,
    pub m_Name: String,
    pub m_Tag: u16,
    pub m_IsActive: bool,
}
impl ClassIdType for GameObject {
    const CLASS_ID: ClassId = ClassId::GameObject;
}
