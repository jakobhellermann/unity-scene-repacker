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

    pub fn TypeTreeGenerator_getLoadedDLLNames(ptr: *mut TypeTreeGeneratorHandle) -> *const c_char;
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
