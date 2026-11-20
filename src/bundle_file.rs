//! Bundling one serialized file of the game on its own.

use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use indexmap::IndexMap;
use log::warn;
use rabex::UnityVersion;
use rabex::files::SerializedFile;
use rabex::files::bundlefile::BundleFileBuilder;
use rabex::files::serializedfile::builder::SerializedFileBuilder;
use rabex::files::serializedfile::{self, FileIdentifier};
use rabex::objects::pptr::PathId;
use rabex::objects::{ClassId, PPtr};
use rabex::typetree::TypeTreeProvider;
use rabex_env::Environment;
use rabex_env::addressables::ArchivePath;
use rabex_env::inline::inline_external;
use rabex_env::resolver::EnvResolver as _;
use rabex_env::streams::MapStreamLocations as _;
use rabex_env::trace_pptr::trace_pptrs_endianned;
use rabex_env::unity::types::{AssetBundle, AssetInfo};
use rustc_hash::FxHashMap;

use crate::dependency_addressables::{
    CATALOG_KEY_MONOSCRIPTS, ExternalPath, bundled, bundled_path, cab_name, game_prefix, is_builtin,
};
use crate::fill_preload_table;
use crate::rewrite_rules::{self, RewriteRule};

pub enum BundleSource<'a> {
    /// A serialized file of the player build, e.g. `resources.assets`
    SerializedFile(&'a str),
    /// One of the game's addressables bundles, relative to the addressables build folder
    AddressablesBundle(&'a str),
}

#[derive(Default)]
pub struct BundleOptions<'a> {
    pub rewrite_rules: &'a [RewriteRule],
    /// Copy the objects of these classes that the file references in other files into it, see
    /// [`inline_external`]
    pub inline_external: &'a [ClassId],
    /// List the assets from the `ResourceManager` in the container, by their `Resources.Load` path
    pub list_resources: bool,
    /// The unity version the bundle should declare, if not the original
    pub target_version: Option<UnityVersion>,
    /// Point the externals at the dependency bundles of the files they name, see
    /// [`crate::dependency_addressables::bundle_game_file`]
    pub redirect_externals: bool,
    /// List the file's MonoScripts in the container, as `monoscripts/<path id>`
    pub list_scripts: bool,
}

/// Bundle `source`, keeping the names of an addressables bundle's entries
pub fn bundle_file(
    env: &Environment,
    source: BundleSource,
    options: &BundleOptions,
) -> Result<BundleFileBuilder> {
    match source {
        BundleSource::SerializedFile(path) => serialized_file_bundle(env, path, options),
        BundleSource::AddressablesBundle(bundle) => addressables_bundle(env, bundle, options),
    }
}

/// Bundle the serialized file `path` of the player build
fn serialized_file_bundle(
    env: &Environment,
    path: &str,
    options: &BundleOptions,
) -> Result<BundleFileBuilder> {
    let handle = env.load_serialized(path)?;
    let (file, data) = (handle.file, handle.data);

    let version = file
        .m_UnityVersion
        .as_ref()
        .context("file has no unity version")?;
    let prefix = game_prefix(env)?;
    let cab = cab_name(&prefix, path);

    let common_offset_map = serializedfile::build_common_offset_map(&env.tpk.inner, version);
    let mut builder = SerializedFileBuilder::from_serialized(
        version,
        file,
        data,
        &env.tpk,
        &common_offset_map,
        file.objects().cloned(),
    );

    rewrite(env, &mut builder, file, data, options)?;
    let streams = relocate_streams(&cab, &mut builder)?;

    if options.redirect_externals {
        for external in &mut builder.serialized.m_Externals {
            if external.pathName.is_empty() || is_builtin(&external.pathName) {
                continue;
            }
            *external = bundled(env, &prefix, &external.pathName)?;
        }
    }

    drop_ref_types(&cab, &mut builder);

    let mut asset_bundle = AssetBundle::asset_base(&cab);
    if options.list_resources {
        add_resources_container(env, path, &mut asset_bundle)?;
        fill_preload_table(&mut asset_bundle, &local_references(env, &builder)?);
    }

    if options.list_scripts {
        for (info, _) in builder.objects.values() {
            if info.m_ClassID == ClassId::MonoScript {
                asset_bundle.m_Container.insert(
                    format!("{CATALOG_KEY_MONOSCRIPTS}/{}", info.m_PathID),
                    AssetInfo::new(PPtr::local(info.m_PathID)),
                );
            }
        }
    }

    let mut bundle = declared_bundle(version, options);
    if needs_split(file) {
        let mut asset_bundle_file =
            SerializedFileBuilder::new(version, &env.tpk, &common_offset_map, true);
        if !asset_bundle.m_Container.is_empty() {
            let file = FileIdentifier::new(bundled_path(&prefix, path, true));
            let data_file = asset_bundle_file.get_or_insert_external_identifier(&file);
            let assets = asset_bundle
                .m_Container
                .values_mut()
                .map(|info| &mut info.asset);
            for pptr in assets.chain(&mut asset_bundle.m_PreloadTable) {
                pptr.m_FileID = data_file;
            }
        }

        asset_bundle_file.add_object_at(1, &asset_bundle)?;
        bundle.add_file(&cab, Cursor::new(asset_bundle_file.write_vec()?))?;
        bundle.add_file(&format!("{cab}.assets"), Cursor::new(builder.write_vec()?))?;
    } else {
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

/// Rebuild the serialized file of the addressables bundle `bundle` and take over its other entries,
/// all under their names
fn addressables_bundle(
    env: &Environment,
    bundle: &str,
    options: &BundleOptions,
) -> Result<BundleFileBuilder> {
    ensure!(
        !options.list_resources,
        "the ResourceManager lists no assets in addressables bundles"
    );

    let reader = env.load_addressables_bundle(bundle)?;
    let [entry] = reader.serialized_files().collect::<Vec<_>>()[..] else {
        bail!("{bundle} doesn't hold exactly one serialized file");
    };
    let data = reader.read_entry(entry)?;
    let unity_version = env.unity_version()?;

    let mut file =
        SerializedFile::from_reader_with_version(&mut Cursor::new(&*data), unity_version)?;
    let version = file
        .m_UnityVersion
        .get_or_insert_with(|| unity_version.clone())
        .clone();

    let common_offset_map = serializedfile::build_common_offset_map(&env.tpk.inner, &version);
    let mut builder = SerializedFileBuilder::from_serialized(
        &version,
        &file,
        &data,
        &env.tpk,
        &common_offset_map,
        file.objects().cloned(),
    );

    rewrite(env, &mut builder, &file, &data, options)?;
    drop_ref_types(&entry.path, &mut builder);

    let mut out = declared_bundle(&version, options);
    out.add_file(&entry.path, Cursor::new(builder.write_vec()?))?;

    for other in reader.files() {
        if other.path != entry.path {
            let data = reader.read_entry(other)?;
            out.add_file_with_flags(&other.path, Cursor::new(&*data), other.flags)?;
        }
    }

    Ok(out)
}

fn rewrite<P: TypeTreeProvider>(
    env: &Environment,
    builder: &mut SerializedFileBuilder<'_, P>,
    file: &SerializedFile,
    data: &[u8],
    options: &BundleOptions,
) -> Result<()> {
    env.fill_typetrees(builder, file, data)?;

    // copying nothing would still trace every object's references
    if !options.inline_external.is_empty() {
        inline_external(env, builder, options.inline_external)?;
    }

    rewrite_rules::apply(env, builder, options.rewrite_rules)
}

/// Check whether path id 1, where Unity expects the `AssetBundle` object, is already used
pub(crate) fn needs_split(file: &SerializedFile) -> bool {
    file.get_object_info(1)
        .is_some_and(|info| info.m_ClassID != ClassId::PreloadData)
}

fn declared_bundle(version: &UnityVersion, options: &BundleOptions) -> BundleFileBuilder {
    BundleFileBuilder::unityfs(7, options.target_version.as_ref().unwrap_or(version))
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

fn add_resources_container(
    env: &Environment,
    path: &str,
    asset_bundle: &mut AssetBundle,
) -> Result<()> {
    let ggm = env.globalgamemanagers()?;
    let resource_manager = env.resource_manager()?;
    for (resource, pptr) in resource_manager.m_Container {
        // local ones are in globalgamemanagers itself
        let Some(index) = pptr.m_FileID.get_externals_index() else {
            continue;
        };
        let file = ggm
            .file
            .m_Externals
            .get(index)
            .with_context(|| format!("resource {resource} is in a missing external"))?;
        if file.pathName == path {
            let info = AssetInfo::new(PPtr::local(pptr.m_PathID));
            asset_bundle.m_Container.insert(resource, info);
        }
    }

    ensure!(
        !asset_bundle.m_Container.is_empty(),
        "the ResourceManager has no assets in {path}"
    );

    Ok(())
}

/// Collect the objects each object of `builder` references in the file itself
fn local_references<P: TypeTreeProvider>(
    env: &Environment,
    builder: &SerializedFileBuilder<'_, P>,
) -> Result<FxHashMap<PathId, Vec<PathId>>> {
    let endianness = builder.serialized.m_Header.m_Endianess;

    let mut references = FxHashMap::default();
    for (&path_id, (info, data)) in &builder.objects {
        let tt = builder
            .serialized
            .get_typetree_for(info, builder.typetree_provider)?;
        let local = trace_pptrs_endianned(&tt, &mut Cursor::new(&**data), endianness, env)
            .with_context(|| format!("object {path_id} ({})", tt.m_Type))?
            .into_iter()
            .filter(|pptr| pptr.m_FileID.is_local() && !pptr.is_null())
            .map(|pptr| pptr.m_PathID)
            .collect();
        references.insert(path_id, local);
    }

    Ok(references)
}
