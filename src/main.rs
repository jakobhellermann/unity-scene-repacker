#![allow(non_snake_case)]
use anyhow::{Context, Result};
use byteorder::LittleEndian;
use indexmap::IndexMap;
use memmap2::Mmap;
use rabex::files::SerializedFile;
use rabex::files::bundlefile::{self, BundleFileHeader, CompressionType};
use rabex::files::serialzedfile::{
    self, FileIdentifier, Guid, ObjectInfo, SerializedFileHeader, SerializedType, TypeTreeProvider,
};
use rabex::objects::ClassId;
use rabex::serde_typetree;
use rabex::tpk::{TpkFile, TpkTypeTreeBlob, UnityVersion};
use rabex::typetree::TypeTreeNode;
use serde::de::{DeserializeSeed, IgnoredAny};
use serde_derive::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let start = Instant::now();

    let preload_path = "/home/jakob/dev/unity/unity-scene-repacker/preloads/hk-palecourt.json";
    let game_directory = Path::new(
        "/home/jakob/.local/share/Steam/steamapps/common/Hollow Knight/hollow_knight_Data",
    );

    let preloads = std::fs::read_to_string(preload_path)
        .with_context(|| format!("couldn't find preload json '{preload_path}'"))?;
    let preloads: IndexMap<String, Vec<String>> = json5::from_str(&preloads)?;

    let mut tpk_file = File::open("lz4.tpk").map_err(|_| {
        anyhow::anyhow!("missing lz4.tpk file, download from https://github.com/AssetRipper/Tpk")
    })?;
    let tpk_file = TpkFile::from_reader(&mut tpk_file)?;
    let tpk = tpk_file.as_type_tree()?.unwrap();
    let typetree_provider = TypeTreeCache::new(&tpk);

    let mut ggm_reader = File::open(game_directory.join("globalgamemanagers"))
        .context("couldn't find globalgamemanagers in game directory")?;
    let ggm = SerializedFile::from_reader(&mut ggm_reader)?;

    let scenes = ggm
        .read_single::<BuildSettings>(ClassId::BuildSettings, &typetree_provider, &mut ggm_reader)?
        .scenes;
    let scenes: HashMap<&str, usize> = scenes
        .iter()
        .enumerate()
        .map(|(i, scene_path)| {
            (
                Path::new(scene_path).file_stem().unwrap().to_str().unwrap(),
                i,
            )
        })
        .collect();

    let mut repack_scenes = Vec::new();
    for (scene_name, paths) in preloads {
        println!("{scene_name}");
        let scene_index = scenes[scene_name.as_str()];
        let path = game_directory.join(format!("level{scene_index}"));

        let (serialized, all_reachable) =
            prune_scene(&scene_name, &path, &typetree_provider, &paths)?;
        repack_scenes.push((scene_name, serialized, path, all_reachable));
    }

    let unity_version: UnityVersion = "2020.2.2f1".parse().unwrap();

    let out_path = Path::new("rust.unity3d");
    let mut out = BufWriter::new(File::create(out_path)?);

    let stats = repack_bundle(
        &mut out,
        &tpk,
        &typetree_provider,
        unity_version,
        &mut repack_scenes,
    )
    .context("trying to repack bundle")?;

    println!(
        "Pruned {} -> {} objects",
        stats.objects_before, stats.objects_after
    );
    println!(
        "{} -> {}",
        friendly_size(stats.size_before),
        friendly_size(stats.size_after)
    );
    println!();

    println!(
        "Repacked into {} ({}) in {:.2?}",
        out_path.display(),
        friendly_size(out.get_ref().metadata()?.len() as usize),
        start.elapsed()
    );

    Ok(())
}

#[inline(never)]
fn prune_scene(
    scene_name: &str,
    path: &Path,
    typetree_provider: impl TypeTreeProvider,
    retain_paths: &[String],
) -> Result<(SerializedFile, BTreeSet<i64>)> {
    let file = File::open(path)?;
    let mut data = Cursor::new(unsafe { Mmap::map(&file)? });

    let serialized = SerializedFile::from_reader(&mut data)
        .with_context(|| format!("Could not parse {scene_name}"))?;

    let scene_lookup = SceneLookup::new(&serialized, typetree_provider, &mut data);
    let new_roots: Vec<_> = retain_paths
        .iter()
        .map(|path| {
            scene_lookup
                .lookup_path_id(&mut data, path)
                .with_context(|| format!("Could not find path '{path}' in {scene_name}"))
        })
        .collect::<Result<_>>()?;

    let mut all_reachable = scene_lookup
        .reachable(&new_roots, &mut data)
        .with_context(|| format!("Could not determine reachable nodes in {scene_name}"))?;

    for settings in serialized.objects_of_class_id(ClassId::RenderSettings) {
        all_reachable.insert(settings.m_PathID);
    }

    Ok((serialized, all_reachable))
}

#[derive(Debug)]
struct Stats {
    objects_before: usize,
    objects_after: usize,
    size_before: usize,
    size_after: usize,
}

fn repack_bundle<W: Write + Seek>(
    writer: W,
    tpk: &TpkTypeTreeBlob,
    typetree_provider: &impl TypeTreeProvider,
    unity_version: UnityVersion,
    scenes: &mut [(String, SerializedFile, PathBuf, BTreeSet<i64>)],
) -> Result<Stats> {
    let mut files = Vec::new();

    let mut stats = Stats {
        objects_before: 0,
        objects_after: 0,
        size_before: 0,
        size_after: 0,
    };

    let common_offset_map = serialzedfile::build_common_offset_map(tpk, unity_version);

    const ASSET_BUNDLE_NAME: &str = "assetBundle";
    let prefix = "bundle";

    let container = scenes
        .iter()
        .map(|(scene_name, ..)| {
            let path = format!("repacker/{prefix}_{scene_name}.unity");
            (path, AssetInfo::default())
        })
        .collect();
    let mut container = Some(container);

    for (name, serialized, path, keep_objects) in scenes {
        let mut sharedassets =
            SerializedFileBuilder::new(unity_version, typetree_provider, &common_offset_map);

        sharedassets.add_object(&PreloadData {
            m_Name: "".into(),
            m_Assets: vec![PPtr {
                m_FileID: 1,
                m_PathID: 10001,
            }],
            ..Default::default()
        })?;

        if let Some(container) = container.take() {
            sharedassets.add_object(&AssetBundle {
                m_Name: ASSET_BUNDLE_NAME.into(),
                m_Container: container,
                m_MainAsset: AssetInfo::default(),
                m_RuntimeCompatibility: 1,
                m_IsStreamedSceneAssetBundle: true,
                m_PathFlags: 7,
                ..Default::default()
            })?;
        }

        let mut out = Cursor::new(Vec::new());
        sharedassets.write(&mut out)?;
        out.set_position(0);

        files.push(Ok((
            format!("BuildPlayer-{prefix}_{name}.sharedAssets"),
            out,
        )));

        let trimmed = {
            let file = File::open(path)?;
            let data = Cursor::new(unsafe { Mmap::map(&file)? });

            stats.objects_before += serialized.m_Objects.len();
            stats.size_before += data.get_ref().len();

            serialized
                .m_Objects
                .retain(|obj| keep_objects.contains(&obj.m_PathID));

            let mut writer = Cursor::new(Vec::new());
            serialzedfile::write_serialized(
                &mut writer,
                serialized,
                data.get_ref(),
                &common_offset_map,
            )?;
            let out = writer.into_inner();

            stats.objects_after += serialized.m_Objects.len();
            stats.size_after += out.len();

            out
        };
        files.push(Ok((
            format!("BuildPlayer-{prefix}_{name}"),
            Cursor::new(trimmed),
        )));
    }

    let header = BundleFileHeader {
        signature: bundlefile::BundleSignature::UnityFS,
        version: 7,
        unity_version: "5.x.x".to_owned(),
        unity_revision: unity_version.to_string(),
        size: 0, // unused
    };

    bundlefile::write_bundle_iter(
        &header,
        writer,
        CompressionType::Lz4hc,
        CompressionType::None, // TODO lz4
        files.into_iter(),
    )?;

    Ok(stats)
}

struct TypeTreeCache<T> {
    inner: T,
    typetree_cache: elsa::FrozenMap<ClassId, Box<Option<TypeTreeNode>>>,
}
impl<T: TypeTreeProvider> TypeTreeCache<T> {
    fn new(inner: T) -> Self {
        Self {
            inner,
            typetree_cache: Default::default(),
        }
    }
}
impl<T: TypeTreeProvider> TypeTreeProvider for TypeTreeCache<T> {
    fn get_typetree_node(
        &self,
        class_id: ClassId,
        target_version: UnityVersion,
    ) -> Option<Cow<'_, TypeTreeNode>> {
        match self.typetree_cache.get(&class_id) {
            Some(value) => value.as_ref().map(Cow::Borrowed),
            None => {
                let tt = self
                    .inner
                    .get_typetree_node(class_id, target_version)
                    .map(Cow::into_owned);
                self.typetree_cache
                    .insert(class_id, Box::new(tt))
                    .as_ref()
                    .map(Cow::Borrowed)
            }
        }
    }
}

struct SerializedFileBuilder<'a, P> {
    unity_version: UnityVersion,
    common_offset_map: &'a HashMap<&'a str, u32>,
    typetree_provider: &'a P,
    next_path_id: i64,
    objects: Vec<(ObjectInfo, Cow<'a, [u8]>)>,

    types: Vec<SerializedType>,
    types_cache: HashMap<ClassId, i32>,
}

impl<'a, P: TypeTreeProvider> SerializedFileBuilder<'a, P> {
    fn new(
        version: UnityVersion,
        typetree_provider: &'a P,
        common_offset_map: &'a HashMap<&'a str, u32>,
    ) -> Self {
        Self {
            unity_version: version,
            typetree_provider,
            common_offset_map,
            next_path_id: 0,
            objects: Vec::new(),
            types: Vec::new(),
            types_cache: HashMap::default(),
        }
    }

    fn add_object<T: serde::Serialize + ClassIdType>(&mut self, object: &T) -> Result<()> {
        let tt = self
            .typetree_provider
            .get_typetree_node(T::CLASS_ID, self.unity_version)
            .unwrap();

        let data = serde_typetree::to_vec::<_, LittleEndian>(&object, &tt)?;

        let type_index = *self.types_cache.entry(T::CLASS_ID).or_insert_with(|| {
            let ty = self
                .typetree_provider
                .get_typetree_node(T::CLASS_ID, self.unity_version)
                .unwrap();
            let type_index = self.types.len();
            self.types
                .push(SerializedType::simple(T::CLASS_ID, Some(ty.into_owned())));
            type_index as i32
        });

        self.objects.push((
            ObjectInfo {
                m_PathID: self.next_path_id,
                m_TypeID: type_index,
                m_ClassID: T::CLASS_ID,
                ..Default::default()
            },
            Cow::Owned(data),
        ));

        self.next_path_id += 1;

        Ok(())
    }

    fn write<W: Write + Seek>(self, writer: W) -> Result<()> {
        let file = SerializedFile {
            m_Header: SerializedFileHeader {
                m_MetadataSize: 0,
                m_FileSize: 0,
                m_Version: 22,
                m_DataOffset: 0,
                m_Endianess: serialzedfile::Endianness::Little,
                m_Reserved: [0, 0, 0],
                unknown: 0,
            },
            m_UnityVersion: Some(self.unity_version),
            m_TargetPlatform: Some(24),
            m_EnableTypeTree: true,
            m_bigIDEnabled: None,
            m_Types: self.types,
            m_Objects: Default::default(),
            m_Objects_lookup: Default::default(),
            m_ScriptTypes: Some(vec![]),
            m_Externals: vec![FileIdentifier {
                tempEmpty: Some("".to_owned()),
                guid: Some(Guid([0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 0, 0, 0, 0])),
                typeId: Some(0),
                pathName: "Library/unity default resources".into(),
            }],
            m_RefTypes: Some(vec![]),
            m_UserInformation: Some("".into()),
        };
        serialzedfile::write_serialized_with(
            writer,
            &file,
            self.common_offset_map,
            self.objects.into_iter(),
        )?;

        Ok(())
    }
}

type PathId = i64;

struct SceneLookup<'a, P> {
    roots: HashMap<String, (PathId, Transform)>,
    serialized: &'a SerializedFile,
    tpk: P,
}

impl<'a, P: TypeTreeProvider> SceneLookup<'a, P> {
    fn new(serialized: &'a SerializedFile, tpk: P, reader: &mut (impl Read + Seek)) -> Self {
        let mut roots = HashMap::new();
        for (name, (path_id, transform)) in serialized
            .objects_of_class_id(ClassId::Transform)
            .filter_map(|info| {
                let transform: Transform = serialized.read(info, &tpk, reader).unwrap();
                let None = transform.m_Father.try_deref(&serialized) else {
                    return None;
                };
                let go = transform.m_GameObject.deref_read(&serialized, &tpk, reader);
                Some((go.m_Name, (info.m_PathID, transform)))
            })
        {
            roots.entry(name).or_insert((path_id, transform));
        }

        SceneLookup {
            roots,
            serialized,
            tpk,
        }
    }

    fn lookup_path_id(&self, reader: &mut (impl Read + Seek), path: &str) -> Option<PathId> {
        self.lookup_path_full(reader, path).map(|(id, _)| id)
    }
    fn lookup_path_full(
        &self,
        reader: &mut (impl Read + Seek),
        path: &str,
    ) -> Option<(i64, Transform)> {
        let mut segments = path.split('/');
        let root_name = segments.next()?;
        let mut current = vec![self.roots.get(root_name)?.clone()];

        for segment in segments {
            let mut found = Vec::new();
            for current in &current {
                for child_pptr in &current.1.m_Children {
                    let child = child_pptr.try_deref_read(self.serialized, &self.tpk, reader)?;
                    let go = child
                        .m_GameObject
                        .deref_read(self.serialized, &self.tpk, reader);

                    if go.m_Name == segment {
                        found.push((child_pptr.m_PathID, child));
                    }
                }
            }

            current = found;
            if current.is_empty() {
                return None;
            }
        }

        current.pop()
    }

    fn reachable(
        &self,
        from: &[PathId],
        reader: &mut (impl Read + Seek),
    ) -> Result<BTreeSet<PathId>> {
        let mut queue: VecDeque<PathId> = VecDeque::new();
        queue.extend(from);

        let mut include = BTreeSet::new();

        while let Some(node) = queue.pop_front() {
            include.insert(node);

            let reachable = match self.reachable_one(node, reader) {
                Ok(reachable) => reachable,
                Err(e) => {
                    eprintln!("[Warn]: {}", e);
                    continue;
                }
            };
            for reachable in reachable {
                if !reachable.is_local() {
                    continue;
                }

                if include.insert(reachable.m_PathID) {
                    queue.push_back(reachable.m_PathID);
                }
            }
        }

        Ok(include)
    }

    fn reachable_one(&self, from: PathId, reader: &mut (impl Read + Seek)) -> Result<Vec<PPtr>> {
        let pptr = PPtr::local(from);
        let info = pptr.deref(self.serialized);

        let tt = self
            .tpk
            .get_typetree_node(info.m_ClassID, self.serialized.m_UnityVersion.unwrap())
            .unwrap();
        reader.seek(std::io::SeekFrom::Start(info.m_Offset as u64))?;
        collect_pptrs(&tt, reader)
    }
}

#[inline(never)]
fn collect_pptrs(tt: &TypeTreeNode, reader: &mut (impl Read + Seek)) -> Result<Vec<PPtr>> {
    let mut deserializer =
        serde_typetree::Deserializer::<_, LittleEndian>::from_reader(reader, &tt);
    let mut output = Vec::new();
    CollectPPtrDeser {
        output: &mut output,
    }
    .deserialize(&mut deserializer)
    .context("Trying to scan for PPtr")?;
    return Ok(output);

    macro_rules! ignore {
        ($name:ident $ty:ident) => {
            fn $name<E: serde::de::Error>(self, _: $ty) -> Result<Self::Value, E> {
                Ok(())
            }
        };
    }

    struct CollectPPtrsVisitor<'a> {
        output: &'a mut Vec<PPtr>,
    }
    impl<'a, 'de> serde::de::Visitor<'de> for CollectPPtrsVisitor<'a> {
        type Value = ();

        ignore!(visit_bool bool);
        ignore!(visit_char char);
        ignore!(visit_u8 u8);
        ignore!(visit_u16 u16);
        ignore!(visit_u32 u32);
        ignore!(visit_u64 u64);
        ignore!(visit_i8 i8);
        ignore!(visit_i16 i16);
        ignore!(visit_i32 i32);
        ignore!(visit_i64 i64);
        ignore!(visit_f32 f32);
        ignore!(visit_f64 f64);
        ignore!(visit_string String);

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut a: A,
        ) -> Result<Self::Value, A::Error> {
            while let Some(_) = a.next_element_seed(CollectPPtrDeser {
                output: self.output,
            })? {}

            Ok(())
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut file_id = None;
            let mut path_id = None;
            let mut others = false;

            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "m_FileID" => file_id = Some(map.next_value::<i32>()?),
                    "m_PathID" => path_id = Some(map.next_value::<i64>()?),
                    "m_Father" => {
                        map.next_value::<IgnoredAny>()?;
                    }
                    _ => {
                        others = true;
                        map.next_value_seed(CollectPPtrDeser {
                            output: self.output,
                        })?;
                    }
                }
            }

            if let (Some(file_id), Some(path_id)) = (file_id, path_id) {
                assert!(!others);
                let pptr = PPtr {
                    m_FileID: file_id,
                    m_PathID: path_id,
                };
                if pptr != PPtr::default() {
                    self.output.push(pptr);
                }
            }

            Ok(())
        }

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("pptr visitor")
        }
    }

    struct CollectPPtrDeser<'a> {
        output: &'a mut Vec<PPtr>,
    }

    impl<'a, 'de> serde::de::DeserializeSeed<'de> for CollectPPtrDeser<'a> {
        type Value = ();

        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(CollectPPtrsVisitor {
                output: self.output,
            })
        }
    }
}

trait ClassIdType {
    const CLASS_ID: ClassId;
}

#[derive(Debug, Deserialize)]
pub struct BuildSettings {
    pub scenes: Vec<String>,
}
impl ClassIdType for BuildSettings {
    const CLASS_ID: ClassId = ClassId::BuildSettings;
}

/// PreloadData is a  class of the Unity engine since version 3.4.0.
#[derive(Debug, Serialize, Default)]
pub struct PreloadData {
    pub m_Name: String,
    pub m_Assets: Vec<PPtr>,
    pub m_Dependencies: Vec<String>,
    pub m_ExplicitDataLayout: bool,
}
impl ClassIdType for PreloadData {
    const CLASS_ID: ClassId = ClassId::PreloadData;
}

#[derive(Debug, Serialize, Default)]
pub struct AssetBundle {
    pub m_Name: String,
    pub m_PreloadTable: Vec<PPtr>,
    pub m_Container: HashMap<String, AssetInfo>,
    pub m_MainAsset: AssetInfo,
    pub m_RuntimeCompatibility: u32,
    pub m_AssetBundleName: String,
    pub m_Dependencies: Vec<String>,
    pub m_IsStreamedSceneAssetBundle: bool,
    pub m_ExplicitDataLayout: i32,
    pub m_PathFlags: i32,
    pub m_SceneHashes: HashMap<String, String>,
}
impl ClassIdType for AssetBundle {
    const CLASS_ID: ClassId = ClassId::AssetBundle;
}

#[derive(Debug, Serialize, Default)]
pub struct AssetInfo {
    pub preloadIndex: i32,
    pub preloadSize: i32,
    pub asset: PPtr,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Transform {
    pub m_Children: Vec<TypedPPtr<Transform>>,
    pub m_Father: TypedPPtr<Transform>,
    pub m_GameObject: TypedPPtr<GameObject>,
}
impl ClassIdType for Transform {
    const CLASS_ID: ClassId = ClassId::Transform;
}

#[derive(Debug, Deserialize)]
pub struct GameObject {
    pub m_Component: Vec<ComponentPair>,
    pub m_IsActive: bool,
    pub m_Layer: u32,
    pub m_Name: String,
    pub m_Tag: u16,
}
impl ClassIdType for GameObject {
    const CLASS_ID: ClassId = ClassId::GameObject;
}

#[derive(Debug, Deserialize)]
pub struct ComponentPair {
    pub component: PPtr,
}

#[derive(Debug, Deserialize)]
pub struct Component {
    pub m_GameObject: TypedPPtr<GameObject>,
}

impl ClassIdType for Component {
    const CLASS_ID: ClassId = ClassId::Component;
}

#[derive(Debug, Serialize, Deserialize, Default, Copy, Clone, PartialEq, Eq)]
pub struct PPtr {
    pub m_FileID: i32,
    pub m_PathID: PathId,
}

impl PPtr {
    pub fn local(path_id: PathId) -> PPtr {
        PPtr {
            m_FileID: 0,
            m_PathID: path_id,
        }
    }
    pub fn is_local(self) -> bool {
        self.m_FileID == 0
    }
    pub fn try_deref(self, serialized: &SerializedFile) -> Option<&ObjectInfo> {
        if self.m_PathID == 0 {
            return None;
        }
        serialized.get_object(self.m_PathID)
    }
    pub fn deref(self, serialized: &SerializedFile) -> &ObjectInfo {
        self.try_deref(serialized).unwrap()
    }
}

#[derive(Deserialize)]
pub struct TypedPPtr<T> {
    pub m_FileID: i32,
    pub m_PathID: i64,
    #[serde(skip)]
    marker: PhantomData<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for TypedPPtr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedPPtr")
            .field("m_FileID", &self.m_FileID)
            .field("m_PathID", &self.m_PathID)
            .finish()
    }
}

impl<T> Copy for TypedPPtr<T> {}
impl<T> Clone for TypedPPtr<T> {
    fn clone(&self) -> Self {
        Self {
            m_FileID: self.m_FileID.clone(),
            m_PathID: self.m_PathID.clone(),
            marker: self.marker,
        }
    }
}

impl<T> TypedPPtr<T> {
    pub fn untyped(self) -> PPtr {
        PPtr {
            m_FileID: self.m_FileID,
            m_PathID: self.m_PathID,
        }
    }
    pub fn try_deref(self, serialized: &SerializedFile) -> Option<&ObjectInfo> {
        self.untyped().try_deref(serialized)
    }
    pub fn deref(self, serialized: &SerializedFile) -> &ObjectInfo {
        self.try_deref(serialized).unwrap()
    }

    pub fn try_deref_read<'de>(
        self,
        serialized: &SerializedFile,
        tpk: impl TypeTreeProvider,
        reader: &mut (impl Read + Seek),
    ) -> Option<T>
    where
        T: serde::Deserialize<'de>,
    {
        let info = self.try_deref(serialized)?;
        Some(serialized.read(info, tpk, reader).unwrap())
    }

    pub fn deref_read<'de>(
        self,
        serialized: &SerializedFile,
        tpk: impl TypeTreeProvider,
        reader: &mut (impl Read + Seek),
    ) -> T
    where
        T: serde::Deserialize<'de>,
    {
        let info = self.try_deref(serialized).unwrap();
        serialized.read(info, tpk, reader).unwrap()
    }
}

fn friendly_size(size: usize) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = size as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{}{}", size as usize, UNITS[unit])
    } else {
        format!("{:.2}{}", size, UNITS[unit])
    }
}
