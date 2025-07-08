use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use paris::info;

fn search_transform(input: &str) -> String {
    input.to_ascii_lowercase().replace(char::is_whitespace, "")
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
                let app = library.apps().filter_map(Result::ok).find(|app| {
                    let name = app.name.as_ref().unwrap_or(&app.install_dir);
                    search_transform(name).contains(&game)
                })?;
                Some((app, library))
            })
            .with_context(|| format!("Didn't find any steam game matching '{game}'"))?
    };

    let install_dir = library.resolve_app_dir(&app);
    let name = app.name.as_ref().unwrap_or(&app.install_dir);
    info!("Detected game '{}' at '{}'", name, install_dir.display());

    find_unity_data_dir(&install_dir)?.with_context(|| {
        format!(
            "Did not find unity 'game_Data' directory in '{}'. Is {} a unity game?",
            install_dir.display(),
            name
        )
    })
}

pub fn find_unity_data_dir(install_dir: &Path) -> Result<Option<PathBuf>> {
    Ok(std::fs::read_dir(install_dir)?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .path()
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.ends_with("_Data"))
                && entry.file_type().is_ok_and(|ty| ty.is_dir())
        })
        .map(|entry| entry.path()))
}
