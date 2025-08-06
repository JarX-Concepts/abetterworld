use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(wasm)");
    cfg_aliases::cfg_aliases! {
        wasm: { all(target_arch = "wasm32", target_os = "unknown") },
    }

    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let is_wasm = target == "wasm32-unknown-unknown";
    let is_ios = target.contains("apple-ios");

    if is_wasm {
        println!("cargo:warning=Skipping Draco for wasm");
        return;
    }

    let draco_src = "./draco";

    // ========== Step 1: Build Draco ========== //
    let mut cmake_cfg = cmake::Config::new(draco_src);

    cmake_cfg
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("DRACO_UNITY_PLUGIN", "OFF")
        .define("DRACO_JS_GLUE", "OFF")
        .define("DRACO_TESTS", "OFF")
        .define("DRACO_ANIMATION_ENCODING", "OFF")
        .define("CMAKE_BUILD_TYPE", "Release")
        .cxxflag("-std=c++17")
        .out_dir(&out_dir);

    if is_ios {
        // Use custom toolchain file and arch
        cmake_cfg
            .define("CMAKE_OSX_SYSROOT", get_apple_sdk_path(&target))
            .define(
                "CMAKE_OSX_ARCHITECTURES",
                if target.starts_with("aarch64") {
                    "arm64"
                } else {
                    "x86_64"
                },
            )
            .define("CMAKE_SYSTEM_NAME", "iOS")
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", "12.0")
            .define(
                "CMAKE_SYSTEM_PROCESSOR",
                if target.starts_with("aarch64") {
                    "arm64"
                } else {
                    "x86_64"
                },
            )
            .generator("Unix Makefiles"); // Ensure portable makefiles for iOS
    }

    let dst = cmake_cfg.build();
    let libdir = format!("{}/lib", dst.display());

    // ========== Step 2: Build C++ wrapper ========== //
    let mut cc_build = cc::Build::new();
    cc_build
        .cpp(true)
        .file("src/decode/native.cc")
        .include(format!("{}/include", dst.display()))
        .include(format!("{}/src", draco_src))
        .flag_if_supported("-std=c++17");

    if is_ios {
        cc_build
            .flag("-isysroot")
            .flag(&get_apple_sdk_path(&target))
            .flag("-arch")
            .flag(if target.starts_with("aarch64") {
                "arm64"
            } else {
                "x86_64"
            });
    }

    cc_build.compile("draco_wrapper");

    // ========== Step 3: Link ========== //
    println!("cargo:rustc-link-search=native={}", libdir);
    println!("cargo:rustc-link-search=native={}", out_dir.display());

    if is_ios {
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=c++"); // libc++ on iOS
    } else if target.contains("apple-darwin") {
        println!("cargo:rustc-link-arg=-Wl,-force_load,{}/libdraco.a", libdir);
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if target.contains("windows") {
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--start-group");
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    println!("cargo:rerun-if-changed=src/decode/native.cc");
    println!("cargo:rerun-if-changed=build.rs");
}

// Helper to get Apple SDK path
fn get_apple_sdk_path(target: &str) -> String {
    use std::process::Command;

    let sdk = if target.contains("sim") || target.contains("x86_64") {
        "iphonesimulator"
    } else {
        "iphoneos"
    };

    let output = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .expect("Failed to get SDK path via xcrun");

    String::from_utf8(output.stdout)
        .expect("Invalid UTF-8 from xcrun")
        .trim()
        .to_string()
}
