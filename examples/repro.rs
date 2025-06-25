#![allow(non_snake_case)]
use rabex::objects::{ClassId, ClassIdType, TypedPPtr};
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::Path;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;

use anyhow::Result;
use rabex::tpk::TpkTypeTreeBlob;
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};

fn main() -> Result<()> {
    let tpk = TpkTypeTreeBlob::embedded();

    // let game_dir = Path::new(
    //     "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data",
    // );
    let game_dir = Path::new(
        "/home/jakob/.local/share/Steam/steamapps/common/Hollow Knight/hollow_knight_Data",
    );

    let level = "level6";
    let object = 11805;

    let env = Environment::new_in(game_dir, &tpk);

    let (file, mut data) = env.load_leaf(level)?;
    let reader = &mut data;

    let backend = GeneratorBackend::AssetsTools;
    let generator = TypeTreeGenerator::new_lib_next_to_exe(file.m_UnityVersion.unwrap(), backend)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;

    let mb_info = file.get_object::<MonoBehaviour>(object, &tpk).unwrap();
    let mb = mb_info.read(reader)?;
    let script = env.deref_read(mb.m_Script, &file, reader)?;

    let base_node = tpk
        .get_typetree_node(ClassId::MonoBehaviour, file.m_UnityVersion.unwrap())
        .unwrap();
    let cache = TypeTreeGeneratorCache::new(generator, base_node);
    let tt = cache.generate(&script.m_AssemblyName, &script.full_name())?;
    println!("{}", tt.dump());

    let x = mb_info.with_typetree::<serde_json::Value>(tt);
    let val = x.read(reader)?;
    dbg!(&val);

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonoBehaviour {
    // pub m_GameObject: TypedPPtr<GameObject>,
    pub m_Enabled: u8,
    pub m_Script: TypedPPtr<MonoScript>,
    pub m_Name: String,
}
impl ClassIdType for MonoBehaviour {
    const CLASS_ID: ClassId = ClassId::MonoBehaviour;
}

#[derive(Debug, Serialize, Deserialize)]
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
