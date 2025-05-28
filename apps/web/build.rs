fn main() {
    let draco_src = "/Users/andrewtosh/proj/draco/src";
    let draco_build = "/Users/andrewtosh/proj/draco/draco_build_web";

    cc::Build::new()
        .cpp(true)
        .file("../../abetterworld/src/ffi/draco_wrapper.cc")
        .include(draco_src) // for draco/compression/*.h
        .include(draco_build) // for generated headers like draco_features.h
        .compile("draco_wrapper");

    println!("cargo:rustc-link-search=native={}", draco_build);
    println!(
        "cargo:rustc-link-search=native={}",
        std::env::var("OUT_DIR").unwrap()
    ); // for libdraco_wrapper.a
    println!("cargo:rustc-link-lib=static=draco_wrapper");
    println!("cargo:rustc-link-lib=static=draco");
}
