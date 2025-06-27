fn main() {
    println!("cargo:rustc-link-search=native=/home/jakob/dev/unity/unity-scene-repacker");
    println!("cargo:rustc-link-lib=TypeTreeGeneratorAPI");
}
