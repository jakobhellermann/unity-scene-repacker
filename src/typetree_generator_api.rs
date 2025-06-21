#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_int};
use std::path::Path;

use rabex::typetree::TypeTreeNode;

mod bindings {
    use std::ffi::{c_char, c_int, c_void};

    #[repr(C)]
    pub struct TypeTreeGeneratorHandle {
        _private: [u8; 0],
    }

    #[link(name = "TypeTreeGeneratorAPI")] // libTypeTreeGenerator.so without 'lib' and '.so'
    unsafe extern "C" {

        pub fn TypeTreeGenerator_init(
            unity_version: *const c_char,
            generator_name: *const c_char,
        ) -> *mut TypeTreeGeneratorHandle;

        pub fn TypeTreeGenerator_loadDLL(
            handle: *mut TypeTreeGeneratorHandle,
            dll_ptr: *const u8,
            dll_len: c_int,
        ) -> c_int;

        pub fn TypeTreeGenerator_del(ptr: *mut TypeTreeGeneratorHandle) -> c_int;

        pub fn TypeTreeGenerator_getLoadedDLLNames(
            ptr: *mut TypeTreeGeneratorHandle,
        ) -> *const c_char;
        pub fn TypeTreeGenerator_generateTreeNodesJson(
            ptr: *mut TypeTreeGeneratorHandle,
            assembly_name: *const c_char,
            full_name: *const c_char,
            json_addr: &mut *mut c_char,
        ) -> c_int;
        pub fn TypeTreeGenerator_generateTreeNodesRaw(
            ptr: *mut TypeTreeGeneratorHandle,
            assembly_name: *const c_char,
            full_name: *const c_char,
            arr_addr: &mut *mut TypeTreeNodeNative,
            arr_length: &mut c_int,
        ) -> c_int;
        pub fn TypeTreeGenerator_getMonoBehaviorDefinitions(
            ptr: *mut TypeTreeGeneratorHandle,
            arr_addr: &mut *const [*const c_char; 2],
            arr_length: &mut c_int,
        ) -> c_int;
        pub fn TypeTreeGenerator_freeMonoBehaviorDefinitions(
            arr_addr: *const [*const c_char; 2],
            arr_length: c_int,
        ) -> c_int;
        pub fn FreeCoTaskMem(ptr: *mut c_void);
    }

    #[derive(Debug)]
    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct TypeTreeNodeNative {
        pub m_Type: *const c_char,
        pub m_Name: *const c_char,
        pub m_Level: c_int,
        pub m_MetaFlag: c_int,
    }
}

pub struct TypeTreeGenerator {
    handle: *mut bindings::TypeTreeGeneratorHandle,
}

pub enum GeneratorBackend {
    AssetStudio,
    AssetsTools,
    AssetRipper,
}

impl TypeTreeGenerator {
    pub fn new(unity_version: &str, backend: GeneratorBackend) -> Result<TypeTreeGenerator, ()> {
        let unity_version = CString::new(unity_version).unwrap();
        let generator_name = match backend {
            GeneratorBackend::AssetStudio => c"AssetStudio",
            GeneratorBackend::AssetsTools => c"AssetsTools",
            GeneratorBackend::AssetRipper => c"AssetRipper",
        };
        let handle = unsafe {
            bindings::TypeTreeGenerator_init(unity_version.as_ptr(), generator_name.as_ptr())
        };
        if handle.is_null() {
            return Err(());
        }
        Ok(TypeTreeGenerator { handle })
    }

    pub fn load_dll_path(&self, path: impl AsRef<Path>) -> Result<(), ()> {
        let data = std::fs::read(&path).map_err(drop)?;
        self.load_dll(&data)
    }

    pub fn load_dll(&self, dll: &[u8]) -> Result<(), ()> {
        let res = unsafe {
            bindings::TypeTreeGenerator_loadDLL(self.handle, dll.as_ptr(), dll.len() as i32)
        };
        if res == 0 { Ok(()) } else { Err(()) }
    }

    pub fn get_monobehaviour_definitions(&self) -> Result<HashMap<String, Vec<String>>, ()> {
        let mut out = std::ptr::null();
        let mut length: c_int = 0;
        let res = unsafe {
            bindings::TypeTreeGenerator_getMonoBehaviorDefinitions(
                self.handle,
                &mut out,
                &mut length,
            )
        };
        if res != 0 {
            return Err(());
        }

        let mut all: HashMap<String, Vec<String>> = HashMap::new();

        unsafe {
            let slice = std::slice::from_raw_parts(out, length as usize);
            for &[module, full_name] in slice {
                let module = CStr::from_ptr(module).to_str().map_err(drop)?.to_owned();
                let full_name = CStr::from_ptr(full_name).to_str().map_err(drop)?.to_owned();
                all.entry(module.clone()).or_default().push(full_name);
            }
        }

        let _ = unsafe { bindings::TypeTreeGenerator_freeMonoBehaviorDefinitions(out, length) };

        Ok(all)
    }

    pub fn generate_typetree_json(&self, assembly: &str, full_name: &str) -> Result<(), ()> {
        let assembly = CString::new(assembly).unwrap();
        let full_name = CString::new(full_name).unwrap();

        let mut json_ptr = std::ptr::null_mut();
        let res = unsafe {
            bindings::TypeTreeGenerator_generateTreeNodesJson(
                self.handle,
                assembly.as_ptr(),
                full_name.as_ptr(),
                &mut json_ptr,
            )
        };
        if res != 0 {
            return Err(());
        }

        let json = unsafe {
            CStr::from_ptr(json_ptr).to_str().map_err(drop)?.to_string();
        };

        let _ = unsafe { bindings::FreeCoTaskMem(json_ptr.cast()) };

        Ok(json)
    }

    pub fn generate_typetree_raw(
        &self,
        assembly: &str,
        full_name: &str,
    ) -> Result<TypeTreeNode, ()> {
        let assembly = CString::new(assembly).unwrap();
        let full_name = CString::new(full_name).unwrap();

        let mut array = std::ptr::null_mut();
        let mut length: c_int = 0;
        let res = unsafe {
            bindings::TypeTreeGenerator_generateTreeNodesRaw(
                self.handle,
                assembly.as_ptr(),
                full_name.as_ptr(),
                &mut array,
                &mut length,
            )
        };
        if res != 0 {
            return Err(());
        }

        let slice = unsafe { std::slice::from_raw_parts(array, length as usize) };

        let items = slice
            .into_iter()
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
        let node = reconstruct(&items);

        let _ = unsafe { bindings::FreeCoTaskMem(array.cast()) };

        Ok(node)
    }
}

fn reconstruct<'a>(all: &[(&'a str, &'a str, u8, i32)]) -> TypeTreeNode {
    let mut stack = Vec::new();

    let mut parent = 0;
    let mut prev = 0;

    let mut children: BTreeMap<usize, Vec<usize>> = Default::default();

    for (node, &(_, _, level, _)) in all.iter().enumerate().skip(1) {
        if level > all[prev].2 {
            stack.push(parent);
            parent = prev;
        } else if level < all[prev].2 {
            while level <= all[parent].2 {
                parent = stack.pop().unwrap();
            }
        }

        children.entry(parent).or_default().push(node);
        prev = node;
    }

    build_node_tree(0, &all, &children)
}

fn build_node_tree(
    index: usize,
    flat_nodes: &[(&str, &str, u8, i32)],
    children_map: &BTreeMap<usize, Vec<usize>>,
) -> TypeTreeNode {
    let &(ty, name, level, flags) = &flat_nodes[index];
    let child_indices = children_map.get(&index);

    let children = match child_indices {
        Some(indices) => indices
            .iter()
            .map(|&child_index| build_node_tree(child_index, flat_nodes, children_map))
            .collect(),
        None => Vec::new(),
    };

    TypeTreeNode {
        m_Type: ty.to_owned(),
        m_Name: name.to_owned(),
        m_Level: level,
        m_MetaFlag: Some(flags),
        children,
        ..Default::default()
    }
}

impl Drop for TypeTreeGenerator {
    fn drop(&mut self) {
        let _ok = unsafe { bindings::TypeTreeGenerator_del(self.handle) };
    }
}
