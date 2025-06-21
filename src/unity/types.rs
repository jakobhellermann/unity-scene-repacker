#![allow(non_snake_case, dead_code)]

use indexmap::IndexMap;
use rabex::objects::pptr::{PPtr, TypedPPtr};
use rabex::objects::{ClassId, ClassIdType};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct BuildSettings {
    pub scenes: Vec<String>,
}
impl ClassIdType for BuildSettings {
    const CLASS_ID: ClassId = ClassId::BuildSettings;
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

#[derive(Debug, Deserialize, Clone)]
pub struct Transform {
    pub m_Children: Vec<TypedPPtr<Transform>>,
    pub m_Father: TypedPPtr<Transform>,
    pub m_GameObject: TypedPPtr<GameObject>,
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
