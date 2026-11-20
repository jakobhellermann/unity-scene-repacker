//! Make an asset bundle loadable in another game, by wrapping and referencing dependency files in bundles, and emitting
//! an addressables catalog.
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use indexmap::{IndexMap, IndexSet};
use rabex::files::SerializedFile;
use rabex::files::bundlefile::CompressionType;
use rabex::files::serializedfile::FileIdentifier;
use rabex::objects::ClassId;
use rabex_env::Environment;
use rabex_env::addressables::ArchivePath;
use rabex_env::addressables::catalog::{
    AddressablesCatalog, AssemblyClass, AssetBundleRequestOptions, CatalogBuilder, assemblies,
};
use rabex_env::resolver::EnvResolver as _;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rustc_hash::FxHashMap;

use crate::bundle_file::{BundleOptions, BundleSource, bundle_file, needs_split};
use crate::rewrite_rules::RewriteRule;
use crate::{RepackScene, get_asset_bundle_object_asset_name, get_scene_bundle_scene_name};

/// Addressables runtime property required to be set to the dependency dir, as `<prefix>.<bundle name>`
const PROPERTY_BUNDLE_DIR_PREFIX: &str = "UnitySceneRepacker";
/// How the dependency bundles are made
const DEPENDENCY: BundleOptions = BundleOptions {
    rewrite_rules: &[],
    list_resources: false,
    inline_external: &[],
    target_version: None,
    redirect_externals: true,
    // so loading all assets loads the MonoScripts: Unity binds MonoBehaviours only to loaded ones
    list_scripts: true,
};

/// Catalog key of the bundles whose MonoScripts must be loaded before an asset is loaded.
pub(crate) const CATALOG_KEY_MONOSCRIPTS: &str = "monoscripts";

/// A game file, as in `FileIdentifier::pathName`.
pub type ExternalPath = String;

pub struct Dependencies {
    prefix: String,
    /// The `AssetBundle.m_Container` key of each repacked object, and the game files it loads objects from.
    containers: IndexMap<String, Vec<ExternalPath>>,
    /// The game files each scene loads objects from
    scenes: IndexMap<String, Vec<ExternalPath>>,
}

impl Dependencies {
    /// The dependencies of the objects in `asset_bundle` and of the scenes in `scene_bundle`
    pub fn new(
        env: &Environment,
        asset_bundle: &[RepackScene],
        scene_bundle: &[RepackScene],
    ) -> Result<Self> {
        let prefix = game_prefix(env)?;

        let scene_lookup = env.build_settings()?.scene_name_lookup();
        let scene_files = |scene: &RepackScene| -> Result<Vec<ExternalPath>> {
            let index = *scene_lookup
                .get(&scene.scene_name)
                .with_context(|| format!("no scene {} in the build settings", scene.scene_name))?;

            Ok(env
                .scene_dependency_files(index)?
                .into_iter()
                .filter(|file| !is_builtin(file))
                .collect())
        };

        let mut containers = IndexMap::new();
        for scene in asset_bundle {
            let files = scene_files(scene)?;
            for (scene_path, _) in &scene.roots {
                let container = get_asset_bundle_object_asset_name(&scene.scene_name, scene_path);
                containers.insert(container, files.clone());
            }
        }
        let scenes = scene_bundle
            .iter()
            .map(|scene| Ok((scene.scene_name.clone(), scene_files(scene)?)))
            .collect::<Result<_>>()?;

        Ok(Dependencies {
            prefix,
            containers,
            scenes,
        })
    }

    pub fn files(&self) -> IndexSet<&str> {
        self.containers
            .values()
            .chain(self.scenes.values())
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// Point the repacked bundle's externals at the bundled dependencies.
    pub fn redirect_externals(
        &self,
        env: &Environment,
        externals: &mut [FileIdentifier],
    ) -> Result<()> {
        let files = self.files();
        for external in externals {
            if external.pathName.is_empty() || is_builtin(&external.pathName) {
                continue;
            }
            ensure!(
                files.contains(external.pathName.as_str()),
                "{} is not loaded by any of the scenes",
                external.pathName
            );
            *external = bundled(env, &self.prefix, &external.pathName)?;
        }

        Ok(())
    }

    /// Write every dependency as `out_dir/deps/<file>.bundle`.
    pub fn write_bundles(
        &self,
        env: &Environment,
        out_dir: &Path,
        compression: CompressionType,
    ) -> Result<()> {
        std::fs::create_dir_all(out_dir.join("deps"))?;

        let files = self.files();
        files.par_iter().try_for_each(|file| -> Result<()> {
            let bundle = bundle_file(env, BundleSource::SerializedFile(file), &DEPENDENCY)
                .with_context(|| format!("bundling {file}"))?;
            bundle.write(
                std::fs::File::create(out_dir.join(bundle_key(file)))?,
                compression,
            )?;

            Ok(())
        })
    }

    /// The catalog of the bundles next to it, keyed by `<name>/<container>` and `<name>/<scene>`.
    pub fn catalog(
        &self,
        env: &Environment,
        name: &str,
        asset_bundle: Option<CatalogBundle>,
        scene_bundle: Option<CatalogBundle>,
    ) -> Result<AddressablesCatalog> {
        ensure!(
            asset_bundle.is_some() || self.containers.is_empty(),
            "repacked objects without an asset bundle"
        );
        ensure!(
            scene_bundle.is_some() || self.scenes.is_empty(),
            "repacked scenes without a scene bundle"
        );

        let bundle_dir = format!("{{{PROPERTY_BUNDLE_DIR_PREFIX}.{name}}}");
        let mut catalog = CatalogBuilder::default();

        let mut locations = IndexMap::new();
        for file in self.files() {
            let key = bundle_key(file);
            let location = catalog.add_bundle(
                &key,
                &format!("{bundle_dir}/{key}"),
                AssetBundleRequestOptions::new(&cab_name(&self.prefix, file)),
            );
            if has_scripts(env.load_serialized(file)?.file) {
                catalog.add_key(CATALOG_KEY_MONOSCRIPTS, location);
            }
            locations.insert(file, location);
        }
        let dependencies = |bundle: usize, files: &[ExternalPath]| {
            std::iter::once(bundle)
                .chain(files.iter().map(|file| locations[file.as_str()]))
                .collect()
        };

        if let Some(bundle) = asset_bundle {
            let location = bundle.add_to(&mut catalog, &bundle_dir);
            for (container, files) in &self.containers {
                catalog.add_asset(
                    &format!("{name}/{container}"),
                    container,
                    AssemblyClass::new(assemblies::CORE_MODULE, "UnityEngine.GameObject"),
                    dependencies(location, files),
                );
            }
        }

        if let Some(bundle) = scene_bundle {
            let location = bundle.add_to(&mut catalog, &bundle_dir);
            for (scene, files) in &self.scenes {
                catalog.add_scene(
                    &format!("{name}/{scene}"),
                    &get_scene_bundle_scene_name(bundle.name, scene),
                    dependencies(location, files),
                );
            }
        }

        Ok(catalog.build(name, &format!("{:032x}", 0)))
    }
}

/// A repacked bundle next to its catalog
pub struct CatalogBundle<'a> {
    pub file: &'a str,
    /// `AssetBundle.m_Name`
    pub name: &'a str,
}

impl CatalogBundle<'_> {
    fn add_to(&self, catalog: &mut CatalogBuilder, bundle_dir: &str) -> usize {
        catalog.add_bundle(
            self.file,
            &format!("{bundle_dir}/{}", self.file),
            AssetBundleRequestOptions::new(self.name),
        )
    }
}

/// The bundle of the game file `path`, as referenced by the bundles and catalog of [`Dependencies`]
pub fn bundle_game_file(
    env: &Environment,
    path: &str,
    compression: CompressionType,
    rewrite_rules: &[RewriteRule],
) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let options = BundleOptions {
        rewrite_rules,
        ..DEPENDENCY
    };
    bundle_file(env, BundleSource::SerializedFile(path), &options)?.write(&mut out, compression)?;
    Ok(out.into_inner())
}

fn bundle_key(file: &str) -> String {
    format!("deps/{file}.bundle")
}

/// The prefix of the bundle entries of `env`'s game files, unique per game bundle version.
pub(crate) fn game_prefix(env: &Environment) -> Result<String> {
    let name = &env.app_info()?.name;
    let version = crate::game_info::game_info(env)?.bundle_version;
    Ok(format!("{name}_{version}")
        .to_lowercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
}

/// The name of the bundle entry holding `file`
pub(crate) fn cab_name(prefix: &str, file: &str) -> String {
    format!("CAB-{prefix}-{}", file.replace('.', "_"))
}

/// The path other files reference `file` by once bundled.
pub(crate) fn bundled_path(prefix: &str, file: &str, split: bool) -> String {
    let cab = cab_name(prefix, file);
    match split {
        false => ArchivePath::same(&cab).to_string(),
        true => ArchivePath::new(&cab, &format!("{cab}.assets")).to_string(),
    }
}

/// The external pointing at `path` once bundled.
pub(crate) fn bundled(env: &Environment, prefix: &str, path: &str) -> Result<FileIdentifier> {
    let split = needs_split(env.load_serialized(path)?.file);
    Ok(FileIdentifier::new(bundled_path(prefix, path, split)))
}

/// Maps the bundle names of `env`'s serialized files back to the files.
pub fn bundled_game_files(env: &Environment) -> Result<FxHashMap<String, ExternalPath>> {
    let prefix = game_prefix(env)?;
    env.game_files
        .serialized_files()?
        .into_iter()
        .map(|path| {
            let path = path
                .into_os_string()
                .into_string()
                .map_err(|path| anyhow::anyhow!("non-utf8 game file {path:?}"))?;
            Ok((cab_name(&prefix, &path), path))
        })
        .collect()
}

fn has_scripts(file: &SerializedFile) -> bool {
    file.objects()
        .any(|info| info.m_ClassID == ClassId::MonoScript)
}

pub(crate) fn is_builtin(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with("unity default resources") || path.ends_with("unity_builtin_extra")
}
