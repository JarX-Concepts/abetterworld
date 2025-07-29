use std::env;
use std::path::PathBuf;

fn main() {
    let draco_src = "./draco"; // relative to your crate
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // 1. Build Draco with cmake
    let dst = cmake::Config::new(draco_src)
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("DRACO_UNITY_PLUGIN", "OFF")
        .define("DRACO_JS_GLUE", "OFF")
        .define("DRACO_TESTS", "OFF")
        .define("DRACO_ANIMATION_ENCODING", "OFF")
        .define("CMAKE_BUILD_TYPE", "Release")
        .out_dir(&out_dir)
        .build();

    // 2. Link libdraco.a (built via cmake)
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=static=draco");

    // 3. Build your C++ wrapper
    cc::Build::new()
        .cpp(true)
        .file("src/decode/native.cc")
        .include(format!("{}/include", dst.display()))
        .include(format!("{}/src", draco_src)) // for headers like draco/compression/...
        .flag_if_supported("-std=c++17")
        .compile("draco_wrapper");

    // 4. Link your wrapper
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=draco_wrapper");
}
