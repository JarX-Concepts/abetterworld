use std::env;
use std::path::PathBuf;

fn main() {
    let draco_src = "./draco";
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Build Draco using cmake
    let dst = cmake::Config::new(draco_src)
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("DRACO_UNITY_PLUGIN", "OFF")
        .define("DRACO_JS_GLUE", "OFF")
        .define("DRACO_TESTS", "OFF")
        .define("DRACO_ANIMATION_ENCODING", "OFF")
        .define("CMAKE_BUILD_TYPE", "Release")
        .cxxflag("-std=c++17") // Force consistent C++ ABI
        .out_dir(&out_dir)
        .build();

    // Link directory for libdraco.a
    println!("cargo:rustc-link-search=native={}/lib", dst.display());

    // Link directory for our draco_wrapper.a
    println!("cargo:rustc-link-search=native={}", out_dir.display());

    // Link order matters: draco first, wrapper second
    println!("cargo:rustc-link-lib=static=draco");
    println!("cargo:rustc-link-lib=static=draco_wrapper");

    // Platform-specific stdlib for C++
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-lib=dylib=stdc++");

    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=dylib=c++");

    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-lib=dylib=stdc++");

    // Compile C++ wrapper
    cc::Build::new()
        .cpp(true)
        .file("src/decode/native.cc")
        .include(format!("{}/include", dst.display())) // generated headers from CMake
        .include(format!("{}/src", draco_src)) // draco/compression/...
        .flag_if_supported("-std=c++17")
        .compile("draco_wrapper");
}
