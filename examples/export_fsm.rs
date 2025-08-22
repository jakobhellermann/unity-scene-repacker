use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rabex::objects::{ClassId, TypedPPtr};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::TypeTreeProvider;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use serde_derive::{Deserialize, Serialize};
use unity_scene_repacker::GameFiles;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};
use unity_scene_repacker::typetree_generator_cache::TypeTreeGeneratorCache;
use unity_scene_repacker::unity::types::{BuildSettings, MonoBehaviour};

fn main() -> Result<()> {
    let include_mbs = ["PlayMakerFSM"];

    let tpk = TpkTypeTreeBlob::embedded();
    let tpk = &TypeTreeCache::new(tpk);

    let mut args = std::env::args().skip(1);
    let game_dir = args
        .next()
        .context("expected path to game dir as first argument")?;
    let game_dir = Path::new(&game_dir);

    let game_files = GameFiles::probe(game_dir)?;
    let mut env = Environment::new(game_files, tpk);

    let (ggm, mut ggm_reader) = env.load_leaf("globalgamemanagers")?;
    let build_settings = ggm
        .find_object_of::<BuildSettings>(tpk)?
        .unwrap()
        .read(&mut ggm_reader)?;
    let scenes: Vec<_> = build_settings.scene_names().collect();
    let unity_version = ggm.m_UnityVersion.unwrap();

    let mb = tpk
        .get_typetree_node(ClassId::MonoBehaviour, unity_version)
        .unwrap();
    let generator =
        TypeTreeGenerator::new_lib_next_to_exe(unity_version, GeneratorBackend::AssetsTools)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;
    env.typetree_generator = TypeTreeGeneratorCache::new(generator, mb.into_owned());

    let mut scene_fsms: BTreeMap<String, Vec<GameFsmInfo>> = BTreeMap::new();

    for file_path in env.resolver.serialized_files()? {
        // let scene = scenes[level_index];

        let (file, mut data) = env.load_leaf(&file_path)?;
        let data = &mut data;
        for mb_obj in file.objects_of::<MonoBehaviour>(tpk)? {
            let mb = mb_obj.read(data)?;
            if mb.m_Script.is_null() {
                continue;
            }
            let script = env.deref_read(mb.m_Script, &file, data)?;
            if include_mbs.contains(&script.m_Name.as_str()) {
                let ty = env
                    .typetree_generator
                    .generate(&script.m_AssemblyName, &script.full_name())?;

                // let fsm = mb_obj.with_typetree::<PlayMakerFSM>(&ty).read(data)?;
                let fsm = mb_obj.with_typetree::<PlayMakerFSM>(&ty).read(data)?;

                let go = mb.m_GameObject.deref_local(&file, tpk)?.read(data)?;
                let path = go.path(&file, data, tpk)?;

                let template = fsm
                    .template
                    .optional()
                    .map(|template| -> Result<_> {
                        let template = env.deref_read_monobehaviour(template, &file, data)?;
                        Ok(FsmTemplateInfo {
                            fsm: template.fsm.name,
                        })
                    })
                    .transpose()?;

                scene_fsms
                    .entry(file_path.display().to_string())
                    .or_default()
                    .push(GameFsmInfo {
                        name: fsm.fsm.name,
                        path,
                        template,
                    });
            }
        }
    }

    for (_, fsms) in &mut scene_fsms {
        fsms.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let json = serde_json::to_string_pretty(&scene_fsms)?;
    println!("{}", json);

    Ok(())
}

#[derive(Serialize)]
struct GameFsmInfo {
    name: String,
    path: String,
    template: Option<FsmTemplateInfo>,
}

#[derive(Serialize)]
struct FsmTemplateInfo {
    fsm: String,
}

#[derive(Deserialize)]
struct PlayMakerFSM {
    fsm: FSM,
    #[serde(rename = "fsmTemplate")]
    template: TypedPPtr<FsmTemplate>,
}

#[derive(Deserialize)]
struct FSM {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct FsmTemplate {
    category: String,
    fsm: FSM,
}
