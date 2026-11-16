fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");

    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("unityscenerepacker")
        .csharp_namespace("UnitySceneRepacker")
        .csharp_class_name("NativeMethods")
        .csharp_class_accessibility("internal")
        .csharp_use_function_pointer(false)
        .generate_csharp_file("dotnet/UnitySceneRepacker.Bindings/NativeMethods.g.cs")
        .expect("failed to generate C# bindings");
}
