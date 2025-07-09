#![allow(non_snake_case, dead_code)]

mod utils;

use std::borrow::Cow;
use std::path::Path;

use indexmap::IndexMap;
use rabex::objects::pptr::{PPtr, TypedPPtr};
use rabex::objects::{ClassId, ClassIdType};
use rustc_hash::FxHashMap;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BuildSettings {
    pub scenes: Vec<String>,
}
impl ClassIdType for BuildSettings {
    const CLASS_ID: ClassId = ClassId::BuildSettings;
}
impl BuildSettings {
    pub fn scene_name_lookup(&self) -> FxHashMap<String, usize> {
        self.scene_names()
            .enumerate()
            .map(|(i, name)| (name.to_owned(), i))
            .collect()
    }

    pub fn scene_names(&self) -> impl Iterator<Item = &str> {
        self.scenes
            .iter()
            .map(|scene_path| Path::new(scene_path).file_stem().unwrap().to_str().unwrap())
    }
}

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
    pub m_Container: IndexMap<String, AssetInfo>,
    pub m_MainAsset: AssetInfo,
    pub m_RuntimeCompatibility: u32,
    pub m_AssetBundleName: String,
    pub m_Dependencies: Vec<String>,
    pub m_IsStreamedSceneAssetBundle: bool,
    pub m_ExplicitDataLayout: i32,
    pub m_PathFlags: i32,
    pub m_SceneHashes: IndexMap<String, String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transform {
    pub m_GameObject: TypedPPtr<GameObject>,
    pub m_LocalRotation: (f32, f32, f32, f32),
    pub m_LocalPosition: (f32, f32, f32),
    pub m_LocalScale: (f32, f32, f32),
    pub m_Children: Vec<TypedPPtr<Transform>>, // TODO recttransform
    pub m_Father: TypedPPtr<Transform>,
}
impl ClassIdType for Transform {
    const CLASS_ID: ClassId = ClassId::Transform;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameObject {
    pub m_Component: Vec<ComponentPair>,
    pub m_Layer: u32,
    pub m_Name: String,
    pub m_Tag: u16,
    pub m_IsActive: bool,
}
impl ClassIdType for GameObject {
    const CLASS_ID: ClassId = ClassId::GameObject;
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct MonoBehaviour {
    pub m_GameObject: TypedPPtr<GameObject>,
    pub m_Enabled: u8,
    pub m_Script: TypedPPtr<MonoScript>,
    pub m_Name: String,
}
impl ClassIdType for MonoBehaviour {
    const CLASS_ID: ClassId = ClassId::MonoBehaviour;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonoScript {
    pub m_Name: String,
    pub m_ExecutionOrder: i32,
    pub m_PropertiesHash: [u8; 16],
    pub m_ClassName: String,
    pub m_Namespace: String,
    pub m_AssemblyName: String,
}
impl MonoScript {
    pub fn full_name(&self) -> Cow<'_, str> {
        match self.m_Namespace.is_empty() {
            true => Cow::Borrowed(&self.m_ClassName),
            false => Cow::Owned(format!("{}.{}", self.m_Namespace, self.m_ClassName)),
        }
    }

    pub fn into_location(self) -> (String, String) {
        let full_name = match self.m_Namespace.is_empty() {
            true => self.m_ClassName,
            false => format!("{}.{}", self.m_Namespace, self.m_ClassName),
        };
        (self.m_AssemblyName, full_name)
    }
}

impl ClassIdType for MonoScript {
    const CLASS_ID: ClassId = ClassId::MonoScript;
}
