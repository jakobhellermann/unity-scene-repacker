use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use rabex::objects::TypedPPtr;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use serde_derive::{Deserialize, Serialize};
use unity_scene_repacker::GameFiles;
use unity_scene_repacker::env::handle::SerializedFileHandle;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::typetree_generator_api::GeneratorBackend;
use unity_scene_repacker::unity::types::{GameObject, MonoBehaviour};

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
    env.load_typetree_generator(GeneratorBackend::default())?;

    let mut scene_fsms: BTreeMap<String, Vec<GameFsmInfo>> = BTreeMap::new();

    for file_path in env.resolver.serialized_files()? {
        let (file, data) = env.load_leaf(&file_path)?;
        let file = SerializedFileHandle::new(&env, &file, data.as_ref());

        for mb_obj in file.objects_of::<MonoBehaviour>()? {
            let Some(script) = mb_obj.mono_script()? else {
                continue;
            };

            if include_mbs.contains(&script.m_Name.as_str()) {
                let fsm = mb_obj.load_typetree_as::<PlayMakerFSM>(&script)?.read()?;
                let go = file.deref_read(fsm.game_object)?;

                let path = go.path(&file.file, &mut file.reader(), tpk)?;

                let template = fsm
                    .template
                    .optional()
                    .map(|template| -> Result<_> {
                        let template = file.deref(template)?.load_typetree()?.read()?;
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

    for fsms in scene_fsms.values_mut() {
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
    #[serde(rename = "m_GameObject")]
    game_object: TypedPPtr<GameObject>,
    fsm: Fsm,
    #[serde(rename = "fsmTemplate")]
    template: TypedPPtr<FsmTemplate>,
}

#[derive(Deserialize)]
struct Fsm {
    name: String,
    // description: String,
}

#[derive(Deserialize)]
struct FsmTemplate {
    // category: String,
    fsm: Fsm,
}
