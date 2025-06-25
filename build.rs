fn main() {
    /*println!(
        "cargo:rustc-link-search=native=/home/jakob/dev/unity/TypeTreeGeneratorAPI/TypeTreeGeneratorAPI/bin/Release/net10.0/linux-x64/publish"
    );*/
    // println!("cargo:rustc-link-search=native=/usr/local/lib");

    println!("cargo:rustc-link-search=native=/home/jakob/dev/unity/unity-scene-repacker");
    println!("cargo:rustc-link-lib=TypeTreeGeneratorAPI");

    // println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
    // println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/libTypeTreeGeneratorAPIÖ.so");
    // println!("cargo:rustc-link-search=native=.");
}
