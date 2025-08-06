// build.rs
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(wasm)");
    cfg_aliases::cfg_aliases! {
        wasm: { all(target_arch = "wasm32", target_os = "unknown") },
    }

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_arch == "wasm32" && target_os == "unknown" {
        // This is a WASM build, skip native build steps
        return;
    }

    // ---------- 1. Build Draco with CMake ----------
    let draco_src = "./draco";
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let dst = cmake::Config::new(draco_src)
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("DRACO_UNITY_PLUGIN", "OFF")
        .define("DRACO_JS_GLUE", "OFF")
        .define("DRACO_TESTS", "OFF")
        .define("DRACO_ANIMATION_ENCODING", "OFF")
        .define("CMAKE_BUILD_TYPE", "Release")
        .cxxflag("-std=c++17")
        .out_dir(&out_dir)
        .build();

    let libdir = format!("{}/lib", dst.display());

    // ---------- 2. Compile our C++ wrapper ----------
    cc::Build::new()
        .cpp(true)
        .file("src/decode/native.cc")
        .include(format!("{}/include", dst.display())) // generated headers
        .include(format!("{}/src", draco_src)) // draco source headers
        .flag_if_supported("-std=c++17")
        .compile("draco_wrapper");

    // ---------- 3. Tell Rust where to find the libraries ----------
    println!("cargo:rustc-link-search=native={}", libdir); // Draco libs
    println!("cargo:rustc-link-search=native={}", out_dir.display()); // wrapper

    // ---------- 4. Platform-specific linking ----------
    if cfg!(target_os = "macos") {
        // macOS: ld64 has no --start-group; use -force_load for circular refs
        println!("cargo:rustc-link-arg=-Wl,-force_load,{}/libdraco.a", libdir);
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=c++"); // libc++
    } else if cfg!(target_os = "windows") {
        // MSVC (link.exe) ignores order for static libs, so just list them
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        // Linux (GNU ld or lld) – group avoids order issues
        println!("cargo:rustc-link-arg=-Wl,--start-group");
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    // ---------- 5. Re-run build.rs if anything here changes ----------
    println!("cargo:rerun-if-changed=src/decode/native.cc");
    println!("cargo:rerun-if-changed=build.rs");
}
