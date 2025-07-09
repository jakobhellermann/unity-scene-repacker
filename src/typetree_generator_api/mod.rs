#![allow(dead_code)]

#[allow(non_snake_case, unsafe_op_in_unsafe_fn)]
mod generated;

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char, c_int};
use std::path::Path;
use std::sync::LazyLock;

use rabex::UnityVersion;
use rabex::typetree::TypeTreeNode;

use crate::typetree_generator_api::generated::{TypeTreeGeneratorAPI, TypeTreeGeneratorHandle};

pub mod cache;

#[rustfmt::skip]
static TYPETREE_GENERATOR_API_LIB: LazyLock<Result<TypeTreeGeneratorAPI,libloading::Error>> = LazyLock::new(|| unsafe {
     TypeTreeGeneratorAPI::new("./libTypeTreeGeneratorAPI.so")
});

pub struct TypeTreeGenerator {
    handle: *mut TypeTreeGeneratorHandle,
    vtable: &'static TypeTreeGeneratorAPI,
}
// The AssetsTools generator API seems to be thread safe
unsafe impl Send for TypeTreeGenerator {}
unsafe impl Sync for TypeTreeGenerator {}

pub enum GeneratorBackend {
    AssetStudio,
    AssetsTools,
    AssetRipper,
}

#[derive(Debug)]
pub enum Error {
    CreationError,
    Status(i32),
    UTF8(std::str::Utf8Error),
    IO(std::io::Error),
    Lib(String),
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::IO(value)
    }
}
impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Error::UTF8(value)
    }
}
impl Error {
    fn from_code(status_code: i32) -> Result<(), Error> {
        if status_code == 0 {
            Ok(())
        } else {
            Err(Error::Status(status_code))
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CreationError => f.write_str("Could not create generator instance"),
            Error::Status(status) => write!(f, "Generator returned status code {status}"),
            Error::UTF8(utf8_error) => write!(f, "Could not decode as utf8: {utf8_error}"),
            Error::IO(error) => write!(f, "IO Error: {error}"),
            Error::Lib(error) => write!(f, "Couldn't load TypeTreeGeneratorAPI library: {error}"),
        }
    }
}
impl std::error::Error for Error {}

impl TypeTreeGenerator {
    pub fn new(
        unity_version: UnityVersion,
        backend: GeneratorBackend,
    ) -> Result<TypeTreeGenerator, Error> {
        let vtable = match &*TYPETREE_GENERATOR_API_LIB {
            Ok(vtable) => vtable,
            Err(e) => return Err(Error::Lib(format!("{e}"))),
        };

        let unity_version = CString::new(unity_version.to_string()).unwrap();
        let generator_name = match backend {
            GeneratorBackend::AssetStudio => c"AssetStudio",
            GeneratorBackend::AssetsTools => c"AssetsTools",
            GeneratorBackend::AssetRipper => c"AssetRipper",
        };
        let handle = unsafe {
            vtable.TypeTreeGenerator_init(unity_version.as_ptr(), generator_name.as_ptr())
        };
        if handle.is_null() {
            return Err(Error::CreationError);
        }
        Ok(TypeTreeGenerator { handle, vtable })
    }

    pub fn load_dll_path(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let data = std::fs::read(&path).map_err(Error::IO)?;
        self.load_dll(&data)
    }

    pub fn load_dll(&self, dll: &[u8]) -> Result<(), Error> {
        let res = unsafe {
            self.vtable
                .TypeTreeGenerator_loadDLL(self.handle, dll.as_ptr(), dll.len() as i32)
        };
        Error::from_code(res)
    }

    pub fn get_loaded_dll_names(&self) -> Result<String, Error> {
        let res = unsafe { self.vtable.TypeTreeGenerator_getLoadedDLLNames(self.handle) };
        let str = unsafe { CString::from_raw(res) };

        Ok(str.into_string().map_err(|e| e.utf8_error())?)
    }

    pub fn load_all_dll_in_dir(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|x| x == "dll")
            {
                self.load_dll_path(entry.path()).unwrap();
            }
        }
        Ok(())
    }

    pub fn get_monobehaviour_definitions(&self) -> Result<HashMap<String, Vec<String>>, Error> {
        let mut out = std::ptr::null_mut::<[*mut c_char; 2]>();
        let mut length: c_int = 0;
        let res = unsafe {
            self.vtable.TypeTreeGenerator_getMonoBehaviorDefinitions(
                self.handle,
                &raw mut out,
                &mut length,
            )
        };
        Error::from_code(res)?;

        let mut all: HashMap<String, Vec<String>> = HashMap::new();

        unsafe {
            let slice = std::slice::from_raw_parts(out, length as usize);
            for &[module, full_name] in slice {
                let module = CStr::from_ptr(module).to_str()?.to_owned();
                let full_name = CStr::from_ptr(full_name).to_str()?.to_owned();
                all.entry(module.clone()).or_default().push(full_name);
            }
        }

        let _ = unsafe {
            self.vtable
                .TypeTreeGenerator_freeMonoBehaviorDefinitions(out, length)
        };

        Ok(all)
    }

    pub fn generate_typetree_json(&self, assembly: &str, full_name: &str) -> Result<String, Error> {
        let assembly = CString::new(assembly).unwrap();
        let full_name = CString::new(full_name).unwrap();

        let mut json_ptr = std::ptr::null_mut();
        let res = unsafe {
            self.vtable.TypeTreeGenerator_generateTreeNodesJson(
                self.handle,
                assembly.as_ptr(),
                full_name.as_ptr(),
                &mut json_ptr,
            )
        };
        Error::from_code(res)?;

        let json = unsafe { CStr::from_ptr(json_ptr).to_str()?.to_string() };

        unsafe { self.vtable.FreeCoTaskMem(json_ptr.cast()) };

        Ok(json)
    }

    pub fn generate_typetree_raw(
        &self,
        base: TypeTreeNode,
        assembly: &str,
        full_name: &str,
    ) -> Result<Option<TypeTreeNode>, Error> {
        let assembly = CString::new(assembly).unwrap();
        let full_name = CString::new(full_name).unwrap();

        let mut array = std::ptr::null_mut();
        let mut length: c_int = 0;
        let res = unsafe {
            self.vtable.TypeTreeGenerator_generateTreeNodesRaw(
                self.handle,
                assembly.as_ptr(),
                full_name.as_ptr(),
                &raw mut array,
                &raw mut length,
            )
        };
        Error::from_code(res)?;

        if array.is_null() {
            return Ok(None);
        }

        let slice = unsafe { std::slice::from_raw_parts(array, length as usize) };

        let items = slice
            .iter()
            .map(|item| {
                let ty = unsafe { CStr::from_ptr(item.m_Type) }.to_str().unwrap();
                let name = unsafe { CStr::from_ptr(item.m_Name) }.to_str().unwrap();
                (
                    ty,
                    name,
                    u8::try_from(item.m_Level).unwrap(),
                    item.m_MetaFlag,
                )
            })
            .collect::<Vec<_>>();
        let mut node = reconstruct_typetree_node(&items);
        node.children.splice(0..0, base.children);

        unsafe { self.vtable.FreeCoTaskMem(array.cast()) };

        Ok(Some(node))
    }
}

pub fn reconstruct_typetree_node<S: AsRef<str>>(flat: &[(S, S, u8, i32)]) -> TypeTreeNode {
    let mut stack = Vec::new();

    let mut parent = 0;
    let mut prev = 0;

    let mut children: BTreeMap<usize, Vec<usize>> = Default::default();

    for (node, &(_, _, level, _)) in flat.iter().enumerate().skip(1) {
        if level > flat[prev].2 {
            stack.push(parent);
            parent = prev;
        } else if level < flat[prev].2 {
            while level <= flat[parent].2 {
                parent = stack.pop().unwrap();
            }
        }

        children.entry(parent).or_default().push(node);
        prev = node;
    }

    build_node_tree(0, flat, &children)
}

fn build_node_tree<S: AsRef<str>>(
    index: usize,
    flat_nodes: &[(S, S, u8, i32)],
    children_map: &BTreeMap<usize, Vec<usize>>,
) -> TypeTreeNode {
    let &(ref ty, ref name, level, flags) = &flat_nodes[index];
    let child_indices = children_map.get(&index);

    let children = match child_indices {
        Some(indices) => indices
            .iter()
            .map(|&child_index| build_node_tree(child_index, flat_nodes, children_map))
            .collect(),
        None => Vec::new(),
    };

    TypeTreeNode {
        m_Type: ty.as_ref().to_owned(),
        m_Name: name.as_ref().to_owned(),
        m_Level: level,
        m_MetaFlag: Some(flags),
        children,
        ..Default::default()
    }
}

impl Drop for TypeTreeGenerator {
    fn drop(&mut self) {
        let _ok = unsafe { self.vtable.TypeTreeGenerator_del(self.handle) };
    }
}
