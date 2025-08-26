use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use rabex::objects::TypedPPtr;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use unity_scene_repacker::GameFiles;
use unity_scene_repacker::env::{EnvResolver, Environment};
use unity_scene_repacker::typetree_generator_api::GeneratorBackend;
use unity_scene_repacker::unity::types::{GameObject, MonoBehaviour, MonoScript};

fn main() -> Result<()> {
    let include_mbs = ["StealthGameMonster", "FlyingMonster"];

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

    let build_settings = env.build_settings()?;
    let scenes: Vec<_> = build_settings.scene_names().collect();

    let mut monsters: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for level_index in env.resolver.level_files()? {
        let scene = scenes[level_index];

        let (file, mut data) = env.load_leaf(format!("level{level_index}"))?;
        let data = &mut Cursor::new(&mut data);

        for mb_obj in file.objects_of::<MonoBehaviour>(tpk)? {
            let Some(script) = file.script_type(mb_obj.info) else {
                continue;
            };
            let script = env.deref_read(script.typed::<MonoScript>(), &file, data)?;

            if include_mbs.contains(&script.m_Name.as_str()) {
                let ty = env
                    .typetree_generator
                    .generate(&script.m_AssemblyName, &script.full_name())?;

                let monster = mb_obj.with_typetree::<StealthGameMonster>(ty).read(data)?;

                if monster.monster_stat.is_null() {
                    continue;
                }

                let monster_stat =
                    env.deref_read_monobehaviour(monster.monster_stat, &file, data)?;
                let (stat_file, mut stat_data) =
                    env.deref_data(monster.monster_stat.untyped(), &file, &mut *data)?;

                let hurt_interrupt_data = monster_stat
                    .hurt_interrupt_data
                    .optional()
                    .map(|hurt_interrupt| {
                        env.deref_read_monobehaviour(hurt_interrupt, stat_file, &mut stat_data)
                    })
                    .transpose()?;
                let level = hurt_interrupt_data.map_or(0, |x| x.monster_level);

                let kind = monster_stat.name.trim_end_matches("_monsterStat");
                let go = monster.game_object.deref_local(&file, tpk)?.read(data)?;

                let path = go.path(&file, data, tpk)?;

                if level >= 0 {
                    monsters
                        .entry(kind.to_owned())
                        .or_default()
                        .push((scene.to_string(), path));
                }
            }
        }
    }

    let mut preloads: BTreeMap<String, Vec<String>> = Default::default();

    for (_, mut paths) in monsters {
        paths.sort();
        paths.dedup_by_key(|x| x.1.clone());

        let (scene, path) = paths.swap_remove(0);
        preloads.entry(scene).or_default().push(path);
    }
    let json = serde_json::to_string_pretty(&preloads)?;
    println!("{}", json);

    Ok(())
}

use serde_derive::Deserialize;

#[derive(Deserialize)]
struct StealthGameMonster {
    #[serde(rename = "m_GameObject")]
    game_object: TypedPPtr<GameObject>,
    #[serde(rename = "_monsterStat")]
    monster_stat: TypedPPtr<MonsterStat>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MonsterStat {
    #[serde(rename = "m_Name")]
    name: String,
    hurt_interrupt_data: TypedPPtr<HurtInterruptData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HurtInterruptData {
    monster_level: i32,
}
