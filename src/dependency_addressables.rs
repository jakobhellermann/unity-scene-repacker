//! Make an asset bundle loadable in another game, by wrapping and referencing dependency files in bundles, and emitting
//! an addressables catalog.
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use indexmap::{IndexMap, IndexSet};
use log::warn;
use rabex::files::SerializedFile;
use rabex::files::bundlefile::{BundleFileBuilder, CompressionType};
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::serializedfile::{self, FileIdentifier};
use rabex::objects::pptr::PathId;
use rabex::objects::{ClassId, PPtr};
use rabex::typetree::TypeTreeProvider;
use rabex_env::Environment;
use rabex_env::addressables::ArchivePath;
use rabex_env::addressables::catalog::{
    AddressablesCatalog, AssemblyClass, AssetBundleRequestOptions, CatalogBuilder, assemblies,
};
use rabex_env::resolver::EnvResolver as _;
use rabex_env::streams::MapStreamLocations as _;
use rabex_env::unity::types::{AssetBundle, AssetInfo, MonoScript};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::{RepackScene, get_asset_bundle_object_asset_name};

/// Addressables runtime property required to be set to the dependency dir, as `<prefix>.<bundle name>`
const PROPERTY_BUNDLE_DIR_PREFIX: &str = "UnitySceneRepacker";
/// Catalog key of the bundles whose MonoScripts must be loaded before an asset is loaded.
const CATALOG_KEY_MONOSCRIPTS: &str = "monoscripts";

/// A game file, as in `FileIdentifier::pathName`.
pub type ExternalPath = String;

pub struct Dependencies {
    prefix: String,
    /// The `AssetBundle.m_Container` key of each repacked object, and the game files it loads objects from.
    containers: IndexMap<String, Vec<ExternalPath>>,
}

impl Dependencies {
    pub fn new(env: &Environment, scenes: &[RepackScene]) -> Result<Self> {
        let prefix = env
            .app_info()?
            .name
            .to_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_");

        let scene_lookup = env.build_settings()?.scene_name_lookup();
        let scene_files = scenes
            .par_iter()
            .map(|scene| -> Result<Vec<ExternalPath>> {
                let index = *scene_lookup.get(&scene.scene_name).with_context(|| {
                    format!("no scene {} in the build settings", scene.scene_name)
                })?;

                Ok(env
                    .scene_dependency_files(index)?
                    .into_iter()
                    .filter(|file| !is_builtin(file))
                    .collect())
            })
            .collect::<Result<Vec<_>>>()?;

        let containers: IndexMap<String, Vec<ExternalPath>> = scenes
            .iter()
            .zip(scene_files)
            .flat_map(|(scene, files)| {
                scene.roots.iter().map(move |(scene_path, _)| {
                    let container =
                        get_asset_bundle_object_asset_name(&scene.scene_name, scene_path);
                    (container, files.clone())
                })
            })
            .collect();

        Ok(Dependencies { prefix, containers })
    }

    /// The name of the bundle entry holding `file`
    fn cab_name(&self, file: &str) -> String {
        format!("CAB-{}-{}", self.prefix, file.replace('.', "_"))
    }

    /// The path other files reference `file` by once bundled.
    fn bundled_path(&self, file: &str, split: bool) -> String {
        let cab = self.cab_name(file);
        match split {
            false => ArchivePath::same(&cab).to_string(),
            true => ArchivePath::new(&cab, &format!("{cab}.assets")).to_string(),
        }
    }

    pub fn files(&self) -> IndexSet<&str> {
        self.containers
            .values()
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// The external pointing at `path` once bundled.
    fn bundled(&self, env: &Environment, path: &str) -> Result<FileIdentifier> {
        let split = needs_split(env.load_serialized(path)?.file);
        Ok(FileIdentifier::new(self.bundled_path(path, split)))
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
            *external = self.bundled(env, &external.pathName)?;
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
            let bundle = self
                .bundle(env, file)
                .with_context(|| format!("bundling {file}"))?;
            bundle.write(
                std::fs::File::create(out_dir.join(bundle_key(file)))?,
                compression,
            )?;

            Ok(())
        })
    }

    pub fn catalog(
        &self,
        env: &Environment,
        main_file: &str,
        main_bundle_name: &str,
    ) -> Result<AddressablesCatalog> {
        let bundle_dir = format!("{{{PROPERTY_BUNDLE_DIR_PREFIX}.{main_bundle_name}}}");
        let mut catalog = CatalogBuilder::default();
        let main = catalog.add_bundle(
            &format!("{main_bundle_name}.bundle"),
            &format!("{bundle_dir}/{main_file}"),
            AssetBundleRequestOptions::new(main_bundle_name),
        );

        let mut locations = IndexMap::new();
        for file in self.files() {
            let key = bundle_key(file);
            let location = catalog.add_bundle(
                &key,
                &format!("{bundle_dir}/{key}"),
                AssetBundleRequestOptions::new(&self.cab_name(file)),
            );
            if has_scripts(env.load_serialized(file)?.file) {
                catalog.add_key(CATALOG_KEY_MONOSCRIPTS, location);
            }
            locations.insert(file, location);
        }

        for (container, files) in &self.containers {
            let dependencies = std::iter::once(main)
                .chain(files.iter().map(|file| locations[file.as_str()]))
                .collect();
            catalog.add_asset(
                &format!("{main_bundle_name}/{container}"),
                container,
                AssemblyClass::new(assemblies::CORE_MODULE, "UnityEngine.GameObject"),
                dependencies,
            );
        }

        let stem = Path::new(main_file)
            .file_stem()
            .context("main bundle file has no name")?
            .to_str()
            .unwrap();
        Ok(catalog.build(stem, &format!("{:032x}", 0)))
    }

    /// Wraps the game file at `path` into a bundle, with its externals and streams pointing into the other bundles
    fn bundle(&self, env: &Environment, path: &str) -> Result<BundleFileBuilder> {
        let handle = env.load_serialized(path)?;
        let split = needs_split(handle.file);
        let version = handle
            .file
            .m_UnityVersion
            .as_ref()
            .context("file has no unity version")?;

        let common_offset_map = serializedfile::build_common_offset_map(&env.tpk.inner, version);
        let mut builder = SerializedFileBuilder::from_serialized(
            version,
            handle.file,
            handle.data,
            &env.tpk,
            &common_offset_map,
            handle.file.objects().cloned(),
        );

        builder.fill_typetrees(version, |script: PPtr| -> Result<_> {
            let script = env.deref_read(
                script.typed::<MonoScript>(),
                handle.file,
                &mut Cursor::new(handle.data),
            )?;
            Ok(env.generate_typetree_for_script(&script)?.clone())
        })?;

        let cab = self.cab_name(path);
        let streams = relocate_streams(&cab, &mut builder)?;

        for external in &mut builder.serialized.m_Externals {
            if external.pathName.is_empty() || is_builtin(&external.pathName) {
                continue;
            }
            *external = self.bundled(env, &external.pathName)?;
        }
        drop_ref_types(path, &mut builder);

        let mut asset_bundle = AssetBundle::asset_base(&cab);
        let mut bundle = BundleFileBuilder::unityfs(7, version);

        if split {
            let mut asset_bundle_file =
                SerializedFileBuilder::new(version, &env.tpk, &common_offset_map, true);

            if has_scripts(handle.file) {
                let data_file = asset_bundle_file.get_or_insert_external_identifier(
                    &FileIdentifier::new(self.bundled_path(path, true)),
                );

                add_script_containers(&mut asset_bundle, &builder, |path_id| {
                    PPtr::new(data_file, path_id)
                });
            }

            asset_bundle_file.add_object_at(1, &asset_bundle)?;
            bundle.add_file(&cab, Cursor::new(asset_bundle_file.write_vec()?))?;
            bundle.add_file(&format!("{cab}.assets"), Cursor::new(builder.write_vec()?))?;
        } else {
            add_script_containers(&mut asset_bundle, &builder, PPtr::local);
            builder.objects.remove(&1);
            builder.add_object_at(1, &asset_bundle)?;
            bundle.add_file(&cab, Cursor::new(builder.write_vec()?))?;
        }

        for (entry, stream_file) in streams {
            let data = env.game_files.read_path(Path::new(&stream_file))?;
            bundle.add_file_with_flags(&entry, Cursor::new(data.as_ref()), 0)?;
        }
        Ok(bundle)
    }
}

fn bundle_key(file: &str) -> String {
    format!("deps/{file}.bundle")
}

/// Unity expects the `AssetBundle` object at path id 1, which might be already used
fn needs_split(file: &SerializedFile) -> bool {
    file.get_object_info(1)
        .is_some_and(|info| info.m_ClassID != ClassId::PreloadData)
}

fn has_scripts(file: &SerializedFile) -> bool {
    file.objects()
        .any(|info| info.m_ClassID == ClassId::MonoScript)
}

fn add_script_containers(
    asset_bundle: &mut AssetBundle,
    builder: &SerializedFileBuilder<impl TypeTreeProvider>,
    pptr: impl Fn(PathId) -> PPtr,
) {
    for (info, _) in builder.objects.values() {
        if info.m_ClassID == ClassId::MonoScript {
            asset_bundle.m_Container.insert(
                format!("{CATALOG_KEY_MONOSCRIPTS}/{}", info.m_PathID),
                AssetInfo::new(pptr(info.m_PathID)),
            );
        }
    }
}

/// Point streamed data into the bundle, returning the bundle entry each used stream file becomes.
fn relocate_streams(
    cab: &str,
    builder: &mut SerializedFileBuilder<impl TypeTreeProvider>,
) -> Result<IndexMap<String, ExternalPath>> {
    let mut entries = IndexMap::new();
    builder.map_stream_locations(|info, location| {
        let extension = Path::new(location.path)
            .extension()
            .and_then(|extension| extension.to_str())
            .with_context(|| {
                format!(
                    "#{} streams from {} without extension",
                    info.m_PathID, location.path
                )
            })?;
        let entry = format!("{cab}.{extension}");
        let stream_file = entries
            .entry(entry.clone())
            .or_insert_with(|| location.path.to_owned());
        ensure!(
            stream_file == location.path,
            "{stream_file} and {} would both be bundled as {entry}",
            location.path
        );
        Ok((ArchivePath::new(cab, &entry).to_string(), location.offset))
    })?;
    Ok(entries)
}

// TODO: generate type trees for [SerializeReference] types
fn drop_ref_types(file: &str, builder: &mut SerializedFileBuilder<impl TypeTreeProvider>) {
    let ref_types = builder.serialized.m_RefTypes.get_or_insert_default();
    if ref_types.iter().all(|ty| ty.m_Type.is_some()) {
        return;
    }
    let types = &builder.serialized.m_Types;
    let affected = builder
        .objects
        .values()
        .filter(|(info, _)| {
            types[info.m_TypeID as usize]
                .m_Type
                .as_ref()
                .is_some_and(|tt| tt.contains_type("ManagedReferencesRegistry"))
        })
        .count();
    warn!(
        "{file}: dropping {} ref types without type trees, {affected} objects with [SerializeReference] fields will not load",
        ref_types.len()
    );
    ref_types.clear();
}

fn is_builtin(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with("unity default resources") || path.ends_with("unity_builtin_extra")
}
