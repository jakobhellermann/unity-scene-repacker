use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use indexmap::IndexMap;
use rabex::files::SerializedFile;
use rabex::objects::{ClassId, ClassIdType, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};

fn main() -> Result<()> {
    let tpk = TpkTypeTreeBlob::embedded();

    let game_dir = Path::new(
        "/home/jakob/.local/share/Steam/steamapps/common/Hollow Knight/hollow_knight_Data",
    );
    let path = game_dir.join("globalgamemanagers");

    let mut data = &mut Cursor::new(std::fs::read(path)?);
    let file = SerializedFile::from_reader(&mut data)?;

    let mut used: HashMap<String, HashSet<String>> = HashMap::new();
    let env = Environment::new_in(game_dir, tpk);
    for file in env.resolver.all_files()? {
        let name = file.file_name().unwrap().to_str().unwrap();
        if name.starts_with("level") {
            let (serialized, mut data) = env.load_leaf(file)?;
            for mb in serialized.objects_of::<MonoBehaviour>(&env.tpk)? {
                let mb = mb.read(&mut data)?;
                let script = env.deref_read(mb.m_Script, &serialized, &mut data)?;
                let full_name = script.full_name().into_owned();
                used.entry(script.m_AssemblyName)
                    .or_default()
                    .insert(full_name);
            }
        }
    }

    let generator =
        TypeTreeGenerator::new(file.m_UnityVersion.unwrap(), GeneratorBackend::AssetStudio)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;

    let definitions = generator.get_monobehaviour_definitions()?;

    let mut output: HashMap<String, IndexMap<String, Vec<_>>> = HashMap::default();
    for (assembly, paths) in definitions {
        let Some(used_paths) = used.get(&assembly) else {
            eprintln!("Unused Assembly: {}", assembly);
            continue;
        };
        eprintln!("Assembly: {}", assembly);

        let mut x = IndexMap::default();
        for path in paths {
            if !used_paths.contains(&path) {
                eprintln!("  Unused Path: {}", path);
                continue;
            }
            eprintln!("  Path: {}", path);
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

    let str = serde_json::to_string_pretty(&output)?;
    let out = lz4_flex::compress_prepend_size(str.as_bytes());
    std::fs::write("dump.json.lz4", out)?;

    Ok(())
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
    // pub m_GameObject: TypedPPtr<GameObject>,
    pub m_Enabled: u8,
    pub m_Script: TypedPPtr<MonoScript>,
    pub m_Name: String,
}

impl ClassIdType for MonoBehaviour {
    const CLASS_ID: ClassId = ClassId::MonoBehaviour;
}
