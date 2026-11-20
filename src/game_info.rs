use anyhow::{Context, Result};
use rabex::objects::{ClassId, ClassIdType};
use rabex_env::Environment;
use serde_derive::Deserialize;

pub struct GameInfo {
    /// `Application.version`
    pub bundle_version: String,
    pub unity_version: String,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct PlayerSettings {
    bundleVersion: String,
}

impl ClassIdType for PlayerSettings {
    const CLASS_ID: ClassId = ClassId::PlayerSettings;
}

pub fn game_info(env: &Environment) -> Result<GameInfo> {
    let player_settings = env
        .globalgamemanagers()?
        .find_object_of::<PlayerSettings>()?
        .context("no PlayerSettings found in globalgamemanagers")?;

    Ok(GameInfo {
        bundle_version: player_settings.bundleVersion,
        unity_version: env.unity_version()?.to_string(),
    })
}
