use anyhow::Result;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use clap_complete::CompletionCandidate;
use rabex_env::resolver::GameFiles;

pub fn complete_steam_game(current: &OsStr) -> Vec<CompletionCandidate> {
    fn complete_steam_game_inner(_: &OsStr) -> Result<Vec<CompletionCandidate>> {
        let steam = steamlocate::SteamDir::locate()?;

        let mut candidates = Vec::new();

        for library in steam.libraries()?.filter_map(Result::ok) {
            for app in library.apps().filter_map(Result::ok) {
                let app_dir = library.resolve_app_dir(&app);
                if GameFiles::probe_dir(&app_dir).is_err() {
                    continue;
                }
                let name = app
                    .name
                    .map(OsString::from)
                    .unwrap_or(Path::new(&app.install_dir).file_name().unwrap().to_owned());
                candidates
                    .push(CompletionCandidate::new(name).help(Some(app.app_id.to_string().into())));
            }
        }

        Ok(candidates)
    }

    complete_steam_game_inner(current).unwrap_or_default()
}
