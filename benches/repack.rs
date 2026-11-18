//! Benchmarks on the files of an installed game, given as `BENCH_GAME_DIR`.

use std::sync::LazyLock;

use rabex::files::bundlefile::CompressionType;
use rabex::tpk::TpkTypeTreeBlob;
use rabex::typetree::typetree_cache::sync::TypeTreeCache;
use rabex_env::Environment;
use rabex_env::resolver::GameFiles;
use unity_scene_repacker::dependency_addressables::bundle_game_file;
use unity_scene_repacker::rewrite_rules::RewriteRule;

fn main() {
    divan::main();
}

static ENV: LazyLock<Environment> = LazyLock::new(|| {
    let game_dir = std::env::var("BENCH_GAME_DIR")
        .expect("BENCH_GAME_DIR must point to a game, e.g. .../Hollow Knight");
    let game_files = GameFiles::probe(game_dir).unwrap();
    Environment::new(game_files, TypeTreeCache::new(TpkTypeTreeBlob::embedded()))
});

const FILES: &[&str] = &[
    "sharedassets6.assets",
    "sharedassets0.assets",
    "resources.assets",
];

static PLAYMAKER_RULES: LazyLock<Vec<RewriteRule>> = LazyLock::new(|| {
    serde_json::from_str(
        r#"[
            { "assembly": "PlayMaker", "script": "PlayMakerFSM", "field": "fsm.preprocessed", "value": false },
            { "assembly": "PlayMaker", "script": "PlayMakerFSM", "field": "eventHandlerComponentsAdded", "value": false }
        ]"#,
    )
    .unwrap()
});

#[divan::bench(args = FILES, sample_count = 5, sample_size = 1)]
fn dependency_bundle_lz4(file: &str) -> Vec<u8> {
    bundle_game_file(&ENV, file, CompressionType::Lz4, &[]).unwrap()
}

#[divan::bench(args = FILES, sample_count = 5, sample_size = 1)]
fn dependency_bundle_lz4_playmaker_rules(file: &str) -> Vec<u8> {
    bundle_game_file(&ENV, file, CompressionType::Lz4, &PLAYMAKER_RULES).unwrap()
}

#[divan::bench(args = FILES, sample_count = 5, sample_size = 1)]
fn dependency_bundle_uncompressed(file: &str) -> Vec<u8> {
    bundle_game_file(&ENV, file, CompressionType::None, &[]).unwrap()
}
