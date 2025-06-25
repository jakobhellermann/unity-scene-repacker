use std::path::Path;

use anyhow::{Context, Result};
use rabex::objects::ClassId;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::GameFiles;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::unity::types::MonoBehaviour;

fn main() -> Result<()> {
    let tpk = TpkTypeTreeBlob::embedded();
    let tpk = &TypeTreeCache::new(tpk);

    let mut args = std::env::args().skip(1);
    let game_dir = args
        .next()
        .context("expected path to game dir as first argument")?;
    let game_dir = Path::new(&game_dir);

    let game_files = GameFiles::probe(game_dir)?;
    let mut env = Environment::new(game_files, tpk);

    let (ggm, _) = env.load_leaf("globalgamemanagers")?;
    let unity_version = ggm.m_UnityVersion.unwrap();

    let mb = tpk
        .get_typetree_node(ClassId::MonoBehaviour, unity_version)
        .unwrap();
    let generator =
        TypeTreeGenerator::new_lib_next_to_exe(unity_version, GeneratorBackend::AssetsTools)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;
    env.typetree_generator = TypeTreeGeneratorCache::new(generator, mb.into_owned());

    /*let (file, mut reader) = env.load_leaf("resources.assets")?;
    let reader = &mut reader;
    let obj = file.get_object::<MonoBehaviour>(73920, tpk)?;*/

    // let (file, mut reader) = env.load_leaf("globalgamemanagers")?;
    let mut with = Vec::new();
    let mut without = Vec::new();

    for path in env.resolver.all_files()? {
        let path = path.file_name().unwrap().to_str().unwrap();
        if path.ends_with("resS") {
            continue;
        }

        let Ok((file, mut reader)) = env.load_leaf(path) else {
            continue;
        };
        let reader = &mut reader;
        println!("{}", &path);

        let mut found = false;
        for mb_obj in file.objects_of::<MonoBehaviour>(tpk)? {
            let mb = mb_obj.read(reader)?;
            if mb.m_Script.is_null() {
                continue;
            }
            let script = env.deref_read(mb.m_Script, &file, reader)?;

            if mb.m_GameObject.is_null() {
                //println!("    {} ({})", mb.m_Name, script.m_ClassName);
                found = true;
            }

            /*if script.full_name() == "FXDealerMaterialTag" {
                let ty = env
                    .typetree_generator
                    .generate(&script.m_AssemblyName, &script.full_name())?;
                let _data = mb_obj.with_typetree::<serde_json::Value>(ty).read(reader)?;

                println!("    {}", mb.m_Name);

                // let tag = PPtr::deserialize(&data["fxMateiralTag"])?;
                // dbg!(tag.file_identifier(&file).map(|x| &x.pathName));
            }*/
        }
        if found {
            with.push(path.to_owned());
        } else {
            without.push(path.to_owned());
        }
    }

    dbg!(&with);
    dbg!(&without);

    Ok(())
}
