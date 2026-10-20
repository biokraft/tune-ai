use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let vendor = manifest.join("../../third_party/open303");
    let shim = manifest.join("shim/shim.cpp");

    let mut sources: Vec<PathBuf> = std::fs::read_dir(&vendor)
        .expect("third_party/open303 must exist")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cpp"))
        .collect();
    sources.sort();

    // fft4g.c is not compiled on its own: rosic_FourierTransformerRadix2.cpp
    // #includes it, so it is built as C++ as part of that translation unit.
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .flag("-std=c++14")
        .flag("-O2")
        .flag("-ffp-contract=off")
        .warnings(false)
        .include(&vendor);
    for source in &sources {
        build.file(source);
    }
    build.file(&shim);
    build.compile("open303_cpp");

    println!("cargo:rerun-if-changed={}", vendor.display());
    println!("cargo:rerun-if-changed={}", shim.display());
    println!("cargo:rerun-if-changed=build.rs");
}
