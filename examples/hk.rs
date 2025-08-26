use anyhow::{Context, Result};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde_derive::{Deserialize, Serialize};
use std::collections::BTreeMap;
use typetree_generator_api::GeneratorBackend;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::env::handle::SerializedFileHandle;
use unity_scene_repacker::unity::types::MonoBehaviour;

fn main() -> Result<()> {
    let game_dir = std::env::args().nth(1).context("missing path to game")?;

    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());
    let mut env = Environment::new_in(game_dir, tpk)?;
    env.load_typetree_generator(GeneratorBackend::default())?;

    let scenes = env.build_settings()?.scene_name_lookup();

    let all = scenes
        .par_iter()
        .map(|(scene_name, scene_idx)| -> Result<_> {
            let mut map = BTreeMap::default();
            let (file, data) = env.load_leaf(format!("level{scene_idx}"))?;
            let file = SerializedFileHandle::new(&env, &file, data.as_ref());

            let mut transitions = Vec::new();

            for mb in file.objects_of::<MonoBehaviour>()? {
                let Some(script) = mb.mono_script()? else {
                    continue;
                };

                if script.full_name() == "TransitionPoint" {
                    let data = mb.cast::<TransitionPoint>().read()?;

                    if !data.targetScene.is_empty() {
                        transitions.push(Transition {
                            to: data.targetScene,
                            entry: data.entryPoint,
                        });
                    }
                }
            }

            map.insert(scene_name.as_str(), transitions);
            Ok(map)
        })
        .try_reduce(BTreeMap::default, |mut a, b| {
            a.extend(b);
            Ok(a)
        })?;

    println!("{}", serde_json::to_string_pretty(&all)?);

    Ok(())
}

#[derive(Debug, Serialize)]
struct Transition {
    to: String,
    entry: String,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
pub struct TransitionPoint {
    // pub m_Name: String,
    // pub isADoor: u8, // bool
    // pub entryDelay: f32,
    // pub alwaysEnterRight: u8, // bool
    // pub alwaysEnterLeft: u8,  // bool
    pub targetScene: String,
    pub entryPoint: String,
    // pub entryOffset: (f32, f32),
    // pub nonHazardGate: u8, // bool
}
