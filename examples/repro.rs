use std::borrow::Cow;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use rabex::files::SerializedFile;
use rabex::files::serializedfile::ObjectRef;
use unity_scene_repacker::typetree_generator_api::cache::TypeTreeGeneratorCache;
use unity_scene_repacker::typetree_generator_api::{GeneratorBackend, TypeTreeGenerator};

fn main() -> Result<()> {
    let game_dir = Path::new(
        "/home/jakob/.local/share/Steam/steamapps/common/Nine Sols-Speedrunpatch/NineSols_Data",
    );
    let path = game_dir.join("globalgamemanagers");

    let mut data = &mut Cursor::new(std::fs::read(path)?);
    let file = SerializedFile::from_reader(&mut data)?;

    let generator =
        TypeTreeGenerator::new(file.m_UnityVersion.unwrap(), GeneratorBackend::AssetStudio)?;
    generator.load_all_dll_in_dir(game_dir.join("Managed"))?;

    let cache = TypeTreeGeneratorCache::new(generator);
    let tt = cache.generate("barbaro.autoattributes.Runtime.dll", "AutoAttributeManager")?;
    dbg!(&tt);

    let info = file.get_object_info(1817).unwrap();
    let obj = ObjectRef::<serde_json::Value>::new(&file, info, Cow::Borrowed(tt));

    let x = obj.read(&mut data)?;

    dbg!(x);

    Ok(())
}
