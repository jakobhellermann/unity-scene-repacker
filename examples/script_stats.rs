use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rabex_env::handle::SerializedFileHandle;
use rabex_env::unity::types::MonoBehaviour;
use rabex_env::{EnvResolver, Environment};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use typetree_generator_api::GeneratorBackend;

fn main() -> Result<()> {
    let game_dir = std::env::args().nth(1).context("missing path to game")?;

    let tpk = TypeTreeCache::new(TpkTypeTreeBlob::embedded());
    let mut env = Environment::new_in(game_dir, tpk)?;
    env.load_typetree_generator(GeneratorBackend::default())?;

    let all = env
        .resolver
        .serialized_files()?
        .into_par_iter()
        .map(|path| -> Result<_> {
            let mut map = BTreeMap::default();
            let (file, data) = env.load_leaf(path)?;
            let file = SerializedFileHandle::new(&env, &file, data.as_ref());

            for mb in file.objects_of::<MonoBehaviour>()? {
                let Some(script) = mb.mono_script()? else {
                    continue;
                };

                *map.entry(script.full_name().into_owned()).or_insert(0) += 1;
            }

            Ok(map)
        })
        .try_reduce(BTreeMap::default, |mut a, b| {
            for (item, count) in b {
                *a.entry(item).or_default() += count;
            }
            Ok(a)
        })?;

    let mut results = all.into_iter().collect::<Vec<_>>();
    results.sort_by_key(|x| std::cmp::Reverse(x.1));

    for (count, name) in results {
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), "{} {}", name, count);
    }

    Ok(())
}
