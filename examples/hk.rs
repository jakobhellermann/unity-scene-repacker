use anyhow::{Context, Result};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde_derive::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Cursor;
use typetree_generator_api::GeneratorBackend;
use unity_scene_repacker::env::Environment;
use unity_scene_repacker::unity::types::{MonoBehaviour, MonoScript};

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
            let data = &mut Cursor::new(data);

            let mut transitions = Vec::new();

            for mb_obj in file.objects_of::<MonoBehaviour>(&env.tpk)? {
                let script_type = file.script_type(mb_obj.info).unwrap().typed::<MonoScript>();
                let script = env.deref_read(script_type, &file, data)?;

                if script.full_name() == "TransitionPoint" {
                    let data = env
                        .load_typetree_as::<TransitionPoint>(&mb_obj, &script)?
                        .read(data)?;

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
