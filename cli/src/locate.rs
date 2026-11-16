use std::path::PathBuf;

use anyhow::{Context, Result};
use paris::info;
use rabex_env::resolver::GameFiles;

fn search_transform(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn locate_steam_game(game: &str) -> Result<PathBuf> {
    let steam = steamlocate::SteamDir::locate()?;

    let game = search_transform(game);

    let (app, library) = if let Ok(app_id) = game.parse() {
        steam
            .find_app(app_id)?
            .with_context(|| format!("Could not locate game with app id {app_id}"))?
    } else {
        steam
            .libraries()?
            .filter_map(Result::ok)
            .find_map(|library| {
                let mut candidates = library
                    .apps()
                    .filter_map(Result::ok)
                    .filter_map(|app| {
                        let name = app.name.as_ref().unwrap_or(&app.install_dir);
                        let name = search_transform(name);
                        let contains = name.contains(&game);
                        contains.then(|| (app, name.len() - game.len()))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_by_key(|&(_, score)| score);
                candidates.into_iter().next().map(|(app, _)| (app, library))
            })
            .with_context(|| format!("Didn't find any steam game matching '{game}'"))?
    };

    let install_dir = library.resolve_app_dir(&app);
    let name = app.name.as_ref().unwrap_or(&app.install_dir);
    info!("Detected game '{}' at '{}'", name, install_dir.display());

    GameFiles::probe_dir(&install_dir)
}
