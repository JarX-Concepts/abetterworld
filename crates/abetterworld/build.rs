use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(wasm)");
    cfg_aliases::cfg_aliases! {
        wasm: { all(target_arch = "wasm32", target_os = "unknown") },
    }

    let target = env::var("TARGET").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let is_wasm = target == "wasm32-unknown-unknown";
    let is_ios = target.contains("apple-ios");
    let is_macos = target.contains("apple-darwin");
    let is_android = target.contains("android");

    println!("is wasm: {is_wasm}");
    println!("is ios: {is_ios}");
    println!("is android: {is_android}");

    if is_wasm {
        println!("cargo:warning=Skipping Draco for wasm");
        return;
    }

    let draco_src = "./draco";

    // ===== CMake (Draco) =====
    let mut cmake_cfg = cmake::Config::new(draco_src);
    cmake_cfg
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("DRACO_UNITY_PLUGIN", "OFF")
        .define("DRACO_JS_GLUE", "OFF")
        .define("DRACO_TESTS", "OFF")
        .define("DRACO_ANIMATION_ENCODING", "OFF")
        .define("DRACO_BUILD_TOOLS", "OFF") // <-- add this
        .define("CMAKE_BUILD_TYPE", "Release")
        .cxxflag("-std=c++17")
        .out_dir(&out_dir);

    if is_ios {
        // Determine sim vs device ONLY for iOS
        let is_sim = target.contains("ios-sim") || target.starts_with("x86_64-apple-ios");
        let (sdk_name, arch) = if is_sim {
            (
                "iphonesimulator",
                if target.starts_with("aarch64") {
                    "arm64"
                } else {
                    "x86_64"
                },
            )
        } else {
            ("iphoneos", "arm64")
        };
        let sdk_path = apple_sdk_path(sdk_name);

        cmake_cfg
            .always_configure(true)
            .define("CMAKE_SYSTEM_NAME", "iOS")
            .define("CMAKE_OSX_SYSROOT", &sdk_path)
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", "12.0")
            .define("CMAKE_OSX_ARCHITECTURES", arch)
            .define("CMAKE_SYSTEM_PROCESSOR", arch)
            .generator("Unix Makefiles");
    } else if is_macos {
        // Make sure cmake picks the right min version (overrides polluted env)
        env::set_var("MACOSX_DEPLOYMENT_TARGET", "15.0");
        // (Optional but helpful) clear stray SDKROOT if set
        env::remove_var("SDKROOT");

        let sdk = apple_sdk_path("macosx");

        cmake_cfg
            .always_configure(true) // <-- ensure CMakeCache is refreshed
            .define("CMAKE_SYSTEM_NAME", "Darwin")
            .define("CMAKE_OSX_SYSROOT", &sdk)
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", "15.0")
            .define("CMAKE_OSX_ARCHITECTURES", "arm64")
            .generator("Unix Makefiles");
    } else if is_android {
        let ndk_home = env::var("ANDROID_NDK_HOME")
            .expect("Set ANDROID_NDK_HOME to build for Android targets");
        let toolchain_file = format!("{}/build/cmake/android.toolchain.cmake", ndk_home);

        let (abi, platform) = match target.as_str() {
            t if t.starts_with("aarch64") => ("arm64-v8a", "android-24"),
            t if t.starts_with("armv7") => ("armeabi-v7a", "android-24"),
            t if t.starts_with("x86_64") => ("x86_64", "android-24"),
            _ => panic!("Unsupported Android target: {target}"),
        };

        cmake_cfg
            .define("CMAKE_SYSTEM_NAME", "Android")
            .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
            .define("CMAKE_ANDROID_NDK", &ndk_home)
            .define("ANDROID_ABI", abi)
            .define("CMAKE_ANDROID_ARCH_ABI", abi)
            .define("ANDROID_PLATFORM", platform)
            .generator("Unix Makefiles");
    }

    let dst = cmake_cfg.build();
    let libdir = format!("{}/lib", dst.display());

    // ===== C++ wrapper (cc) =====
    let mut cc_build = cc::Build::new();
    cc_build
        .cpp(true)
        .file("src/decode/native.cc")
        .include(format!("{}/include", dst.display()))
        .include(format!("{}/src", draco_src))
        .flag_if_supported("-std=c++17");

    if is_ios {
        // recompute minimal bits for cc flags
        let is_sim = target.contains("ios-sim") || target.starts_with("x86_64-apple-ios");
        let (sdk_name, arch) = if is_sim {
            (
                "iphonesimulator",
                if target.starts_with("aarch64") {
                    "arm64"
                } else {
                    "x86_64"
                },
            )
        } else {
            ("iphoneos", "arm64")
        };
        let sdk_path = apple_sdk_path(sdk_name);

        cc_build
            .flag("-isysroot")
            .flag(&sdk_path)
            .flag("-arch")
            .flag(arch);
    } else if is_macos {
        let sdk = apple_sdk_path("macosx");
        cc_build
            .flag("-isysroot")
            .flag(&sdk)
            .flag("-arch")
            .flag("arm64")
            .flag("-stdlib=libc++") // <-- add
            .flag("-mmacosx-version-min=15.0"); // <-- add
    } else if is_android {
        let cc_target = if target.starts_with("aarch64") {
            "aarch64-linux-android"
        } else if target.starts_with("armv7") {
            "armv7-linux-androideabi"
        } else if target.starts_with("x86_64") {
            "x86_64-linux-android"
        } else {
            panic!("Unsupported Android target: {target}");
        };
        cc_build
            .flag("-DANDROID")
            .flag("-fPIC")
            .flag(&format!("--target={cc_target}"));
    }

    cc_build.compile("draco_wrapper");

    // ===== Link =====
    println!("cargo:rustc-link-search=native={}", libdir);
    println!("cargo:rustc-link-search=native={}", out_dir.display());

    if is_ios {
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=c++");
    } else if is_macos {
        println!("cargo:rustc-link-arg=-Wl,-force_load,{}/libdraco.a", libdir);
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if target.contains("windows") {
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        // Linux/Android
        println!("cargo:rustc-link-arg=-Wl,--start-group");
        println!("cargo:rustc-link-lib=static=draco_wrapper");
        println!("cargo:rustc-link-lib=static=draco");
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    println!("cargo:rerun-if-changed=src/decode/native.cc");
    println!("cargo:rerun-if-changed=build.rs");
}

fn apple_sdk_path(sdk: &str) -> String {
    let out = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .expect("xcrun failed");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
