/* automatically generated by rust-bindgen 0.72.0 */

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TypeTreeGeneratorHandle {
    _unused: [u8; 0],
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TypeTreeNodeNative {
    pub m_Type: *mut ::std::os::raw::c_char,
    pub m_Name: *mut ::std::os::raw::c_char,
    pub m_Level: ::std::os::raw::c_int,
    pub m_MetaFlag: ::std::os::raw::c_int,
}
#[allow(clippy::unnecessary_operation, clippy::identity_op)]
const _: () = {
    ["Size of TypeTreeNodeNative"][::std::mem::size_of::<TypeTreeNodeNative>() - 24usize];
    ["Alignment of TypeTreeNodeNative"][::std::mem::align_of::<TypeTreeNodeNative>() - 8usize];
    ["Offset of field: TypeTreeNodeNative::m_Type"]
        [::std::mem::offset_of!(TypeTreeNodeNative, m_Type) - 0usize];
    ["Offset of field: TypeTreeNodeNative::m_Name"]
        [::std::mem::offset_of!(TypeTreeNodeNative, m_Name) - 8usize];
    ["Offset of field: TypeTreeNodeNative::m_Level"]
        [::std::mem::offset_of!(TypeTreeNodeNative, m_Level) - 16usize];
    ["Offset of field: TypeTreeNodeNative::m_MetaFlag"]
        [::std::mem::offset_of!(TypeTreeNodeNative, m_MetaFlag) - 20usize];
};
pub struct TypeTreeGeneratorAPI {
    __library: ::libloading::Library,
    pub TypeTreeGenerator_init: unsafe extern "C" fn(
        unity_version: *const ::std::os::raw::c_char,
        generator_name: *const ::std::os::raw::c_char,
    ) -> *mut TypeTreeGeneratorHandle,
    pub TypeTreeGenerator_loadDLL: unsafe extern "C" fn(
        handle: *mut TypeTreeGeneratorHandle,
        dll_ptr: *const ::std::os::raw::c_uchar,
        dll_len: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int,
    pub TypeTreeGenerator_getLoadedDLLNames:
        unsafe extern "C" fn(handle: *mut TypeTreeGeneratorHandle) -> *mut ::std::os::raw::c_char,
    pub TypeTreeGenerator_generateTreeNodesJson: unsafe extern "C" fn(
        handle: *mut TypeTreeGeneratorHandle,
        assembly_name: *const ::std::os::raw::c_char,
        full_name: *const ::std::os::raw::c_char,
        json_addr: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int,
    pub TypeTreeGenerator_generateTreeNodesRaw: unsafe extern "C" fn(
        handle: *mut TypeTreeGeneratorHandle,
        assembly_name: *const ::std::os::raw::c_char,
        full_name: *const ::std::os::raw::c_char,
        arr_addr: *mut *mut TypeTreeNodeNative,
        arr_length: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int,
    pub TypeTreeGenerator_getMonoBehaviorDefinitions: unsafe extern "C" fn(
        handle: *mut TypeTreeGeneratorHandle,
        arr_addr: *mut *mut [*mut ::std::os::raw::c_char; 2usize],
        arr_length: *mut ::std::os::raw::c_int,
    )
        -> ::std::os::raw::c_int,
    pub TypeTreeGenerator_freeMonoBehaviorDefinitions:
        unsafe extern "C" fn(
            arr_addr: *mut [*mut ::std::os::raw::c_char; 2usize],
            arr_length: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    pub TypeTreeGenerator_del:
        unsafe extern "C" fn(handle: *mut TypeTreeGeneratorHandle) -> ::std::os::raw::c_int,
    pub FreeCoTaskMem: unsafe extern "C" fn(ptr: *mut ::std::os::raw::c_void),
}
impl TypeTreeGeneratorAPI {
    pub unsafe fn new<P>(path: P) -> Result<Self, ::libloading::Error>
    where
        P: AsRef<::std::ffi::OsStr>,
    {
        let library = ::libloading::Library::new(path)?;
        Self::from_library(library)
    }
    pub unsafe fn from_library<L>(library: L) -> Result<Self, ::libloading::Error>
    where
        L: Into<::libloading::Library>,
    {
        let __library = library.into();
        let TypeTreeGenerator_init = __library.get(b"TypeTreeGenerator_init\0").map(|sym| *sym)?;
        let TypeTreeGenerator_loadDLL = __library
            .get(b"TypeTreeGenerator_loadDLL\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_getLoadedDLLNames = __library
            .get(b"TypeTreeGenerator_getLoadedDLLNames\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_generateTreeNodesJson = __library
            .get(b"TypeTreeGenerator_generateTreeNodesJson\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_generateTreeNodesRaw = __library
            .get(b"TypeTreeGenerator_generateTreeNodesRaw\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_getMonoBehaviorDefinitions = __library
            .get(b"TypeTreeGenerator_getMonoBehaviorDefinitions\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_freeMonoBehaviorDefinitions = __library
            .get(b"TypeTreeGenerator_freeMonoBehaviorDefinitions\0")
            .map(|sym| *sym)?;
        let TypeTreeGenerator_del = __library.get(b"TypeTreeGenerator_del\0").map(|sym| *sym)?;
        let FreeCoTaskMem = __library.get(b"FreeCoTaskMem\0").map(|sym| *sym)?;
        Ok(TypeTreeGeneratorAPI {
            __library,
            TypeTreeGenerator_init,
            TypeTreeGenerator_loadDLL,
            TypeTreeGenerator_getLoadedDLLNames,
            TypeTreeGenerator_generateTreeNodesJson,
            TypeTreeGenerator_generateTreeNodesRaw,
            TypeTreeGenerator_getMonoBehaviorDefinitions,
            TypeTreeGenerator_freeMonoBehaviorDefinitions,
            TypeTreeGenerator_del,
            FreeCoTaskMem,
        })
    }
    pub unsafe fn TypeTreeGenerator_init(
        &self,
        unity_version: *const ::std::os::raw::c_char,
        generator_name: *const ::std::os::raw::c_char,
    ) -> *mut TypeTreeGeneratorHandle {
        (self.TypeTreeGenerator_init)(unity_version, generator_name)
    }
    pub unsafe fn TypeTreeGenerator_loadDLL(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
        dll_ptr: *const ::std::os::raw::c_uchar,
        dll_len: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_loadDLL)(handle, dll_ptr, dll_len)
    }
    pub unsafe fn TypeTreeGenerator_getLoadedDLLNames(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
    ) -> *mut ::std::os::raw::c_char {
        (self.TypeTreeGenerator_getLoadedDLLNames)(handle)
    }
    pub unsafe fn TypeTreeGenerator_generateTreeNodesJson(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
        assembly_name: *const ::std::os::raw::c_char,
        full_name: *const ::std::os::raw::c_char,
        json_addr: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_generateTreeNodesJson)(handle, assembly_name, full_name, json_addr)
    }
    pub unsafe fn TypeTreeGenerator_generateTreeNodesRaw(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
        assembly_name: *const ::std::os::raw::c_char,
        full_name: *const ::std::os::raw::c_char,
        arr_addr: *mut *mut TypeTreeNodeNative,
        arr_length: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_generateTreeNodesRaw)(
            handle,
            assembly_name,
            full_name,
            arr_addr,
            arr_length,
        )
    }
    pub unsafe fn TypeTreeGenerator_getMonoBehaviorDefinitions(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
        arr_addr: *mut *mut [*mut ::std::os::raw::c_char; 2usize],
        arr_length: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_getMonoBehaviorDefinitions)(handle, arr_addr, arr_length)
    }
    pub unsafe fn TypeTreeGenerator_freeMonoBehaviorDefinitions(
        &self,
        arr_addr: *mut [*mut ::std::os::raw::c_char; 2usize],
        arr_length: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_freeMonoBehaviorDefinitions)(arr_addr, arr_length)
    }
    pub unsafe fn TypeTreeGenerator_del(
        &self,
        handle: *mut TypeTreeGeneratorHandle,
    ) -> ::std::os::raw::c_int {
        (self.TypeTreeGenerator_del)(handle)
    }
    pub unsafe fn FreeCoTaskMem(&self, ptr: *mut ::std::os::raw::c_void) {
        (self.FreeCoTaskMem)(ptr)
    }
}
