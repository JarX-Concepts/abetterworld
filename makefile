.PHONY: build-web run-web

build-web-debug:
	cargo build --target wasm32-unknown-unknown --manifest-path bindings/web/Cargo.toml
	wasm-bindgen target/wasm32-unknown-unknown/debug/abw_web.wasm --out-dir bindings/web/debug/pkg --target web

build-web-release:
	cargo build --target wasm32-unknown-unknown --manifest-path bindings/web/Cargo.toml --release
	wasm-bindgen target/wasm32-unknown-unknown/release/abw_web.wasm --out-dir bindings/web/release/pkg --target web


start-web-debug:
	$(MAKE) build-web-debug
	mkdir -p examples/web/pkg
	cp -r bindings/web/debug/pkg/* examples/web/pkg
	cd examples/web && python3 -m http.server 8080

start-web-release:
	$(MAKE) build-web-release
	mkdir -p examples/web/pkg
	cp -r bindings/web/release/pkg/* examples/web/pkg
	cd examples/web && python3 -m http.server 8080

test-web:
	cd crates/abetterworld && wasm-pack test --firefox --headless

.PHONY: build-ios-debug build-ios-release

CRATE_IOS = abw_ios

build-ios-debug:
	$(MAKE) build-ios-xcframework BUILD_TYPE=debug
	$(MAKE) build-ios-cbindgen

build-ios-release:
	$(MAKE) build-ios-xcframework BUILD_TYPE=release
	$(MAKE) build-ios-cbindgen

ifeq ($(BUILD_TYPE),release)
CARGO_FLAGS := --release
BUILD_DIR := release
else
CARGO_FLAGS :=
BUILD_DIR := debug
endif

build-ios-cbindgen:
	@echo "🔨 Building iOS header with cbindgen..."
	@command -v cbindgen >/dev/null 2>&1 || cargo install --locked cbindgen
	mkdir -p target/headers

	# Generate a single header we’ll attach to all slices
	cbindgen bindings/ios -o target/headers/abetterworld_ios.h
	@echo "✅ Header generated: target/headers/abetterworld_ios.h"


build-ios-xcframework:
	@echo "🔨 Building $(BUILD_TYPE) for iOS + Simulator (xcframework w/ lipo for sim)..."

	# Device (arm64 iPhone)
	cargo build --package $(CRATE_IOS) --target aarch64-apple-ios $(CARGO_FLAGS)
	# Simulator (arm64 + x86_64)
	cargo build --package $(CRATE_IOS) --target aarch64-apple-ios-sim $(CARGO_FLAGS)
	cargo build --package $(CRATE_IOS) --target x86_64-apple-ios $(CARGO_FLAGS)

	@echo "🧬 Creating fat simulator static lib..."
	mkdir -p target/universal/$(BUILD_DIR)
	lipo -create \
		target/aarch64-apple-ios-sim/$(BUILD_DIR)/lib$(CRATE_IOS).a \
		target/x86_64-apple-ios/$(BUILD_DIR)/lib$(CRATE_IOS).a \
		-output target/universal/$(BUILD_DIR)/lib$(CRATE_IOS)_sim.a

	@echo "📦 Creating xcframework..."
	mkdir -p target/xcframework/$(BUILD_DIR)
	rm -rf target/xcframework/$(BUILD_DIR)/$(CRATE_IOS).xcframework

	xcodebuild -create-xcframework \
		-library target/aarch64-apple-ios/$(BUILD_DIR)/lib$(CRATE_IOS).a \
		-library target/universal/$(BUILD_DIR)/lib$(CRATE_IOS)_sim.a \
		-output target/xcframework/$(BUILD_DIR)/$(CRATE_IOS).xcframework

	@echo "✅ XCFramework created: target/xcframework/$(BUILD_DIR)/$(CRATE_IOS).xcframework"

.PHONY: build-android-debug build-android-release build-android build-android-cbindgen \
        copy-android-libs clean-android-jniLibs

# ---- Config ----
ANDROID_TARGETS = \
	aarch64-linux-android \
	armv7-linux-androideabi \
	x86_64-linux-android

CRATE_ANDROID := abw_android
BUILD_TYPE    ?= debug

# Your Android project/module path (matches screenshot)
ANDROID_PROJECT := bindings/android/android
ANDROID_APP     := examples/android/
APP_JNILIBS     := $(ANDROID_APP)/src/main/jniLibs

ANDROID_ENV := env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET -u CPATH -u C_INCLUDE_PATH -u CPLUS_INCLUDE_PATH -u CFLAGS -u CPPFLAGS

# Map rust triple -> Android ABI dir
define MAP_TRIPLE_TO_ABI
case "$1" in \
  aarch64-linux-android)   echo "arm64-v8a" ;; \
  armv7-linux-androideabi) echo "armeabi-v7a" ;; \
  x86_64-linux-android)    echo "x86_64" ;; \
  *) echo "unknown"; exit 1 ;; \
esac
endef

# Optional: strip release libs a bit (uses NDK llvm-strip if available)
STRIP ?=
ifeq ($(BUILD_TYPE),release)
  STRIP := $${ANDROID_NDK_HOME:+$${ANDROID_NDK_HOME}/toolchains/llvm/prebuilt/darwin-arm64/bin/llvm-strip}
endif

# --- NDK autodetect (works with darwin-x86_64, darwin-arm64, darwin, etc.) ---
NDK := $(or $(ANDROID_NDK_HOME),$(ANDROID_NDK_ROOT),$(ANDROID_NDK))
NDK_PREBUILT := $(or \
  $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin-arm64), \
  $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin-x86_64), \
  $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin), \
  $(firstword $(wildcard $(NDK)/toolchains/llvm/prebuilt/*)) \
)

# --- Detect if we're building any Android target on this invocation ---
ANDROID_GOALS := build-android build-android-debug build-android-release build-android-cbindgen copy-android-libs
NEEDS_ANDROID := $(filter $(ANDROID_GOALS),$(MAKECMDGOALS))

# --- Only do NDK detection when an Android goal is present ---
ifeq ($(NEEDS_ANDROID),)
  # Not an Android build; skip NDK checks/vars entirely
else
  # NDK autodetect (runs only for Android goals)
  NDK := $(or $(ANDROID_NDK_HOME),$(ANDROID_NDK_ROOT),$(ANDROID_NDK))
  NDK_PREBUILT := $(or \
    $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin-arm64), \
    $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin-x86_64), \
    $(wildcard $(NDK)/toolchains/llvm/prebuilt/darwin), \
    $(firstword $(wildcard $(NDK)/toolchains/llvm/prebuilt/*)) \
  )
  ifeq ($(NDK_PREBUILT),)
    $(error ❌ Could not find NDK prebuilt under $(NDK)/toolchains/llvm/prebuilt/ (set ANDROID_NDK_HOME))
  endif

  # Optional: strip in release only
  ifeq ($(BUILD_TYPE),release)
    STRIP := $(NDK_PREBUILT)/bin/llvm-strip
  endif
endif

# Optional: strip release libs (only if tool exists)
STRIP ?=
ifeq ($(BUILD_TYPE),release)
  STRIP := $(NDK_PREBUILT)/bin/llvm-strip
endif

# ---- Public targets ----
build-android-debug:
	$(MAKE) build-android BUILD_TYPE=debug

build-android-release:
	$(MAKE) build-android BUILD_TYPE=release

# ---- Rust build + copy ----
build-android:
	@echo "📱 Building $(BUILD_TYPE) for Android targets..."
	@for TARGET in $(ANDROID_TARGETS); do \
		echo "🔨 cargo ndk for $$TARGET..."; \
		$(ANDROID_ENV) cargo ndk -t $$TARGET -o target/android/$(BUILD_TYPE)/ \
			build $(CARGO_BUILD_FLAGS) --package $(CRATE_ANDROID); \
	done
	$(MAKE) copy-android-libs BUILD_TYPE=$(BUILD_TYPE)

# ---- Housekeeping ----
# --- copy libs: Rust .so + libc++_shared.so ---
copy-android-libs: clean-android-jniLibs
	@echo "📦 Copying .so files into $(APP_JNILIBS) (including libc++_shared.so)..."
	@for target in $(ANDROID_TARGETS); do \
		case $$target in \
			aarch64-linux-android)   abi="arm64-v8a";   ndk_triple="aarch64-linux-android" ;; \
			armv7-linux-androideabi) abi="armeabi-v7a"; ndk_triple="arm-linux-androideabi" ;; \
			x86_64-linux-android)    abi="x86_64";      ndk_triple="x86_64-linux-android" ;; \
			*) echo "Unknown target: $$target"; exit 1 ;; \
		esac; \
		dst="$(APP_JNILIBS)/$$abi"; \
		src_rust="target/android/$(BUILD_TYPE)/$$abi/lib$(CRATE_ANDROID).so"; \
		echo "  → $$abi"; \
		mkdir -p "$$dst"; \
		if [ -f "$$src_rust" ]; then \
			cp -f "$$src_rust" "$$dst/"; \
			if [ -n "$(STRIP)" ] && [ -x "$(STRIP)" ]; then $(STRIP) -S "$$dst/lib$(CRATE_ANDROID).so" || true; fi; \
		else \
			echo "    ⚠️  missing $$src_rust"; \
		fi; \
		src_libcxx="$(NDK_PREBUILT)/sysroot/usr/lib/$$ndk_triple/libc++_shared.so"; \
		if [ ! -f "$$src_libcxx" ]; then \
			src_libcxx=$$(find "$(NDK_PREBUILT)/sysroot/usr/lib/$$ndk_triple" -name libc++_shared.so 2>/dev/null | head -n1); \
		fi; \
		if [ -n "$$src_libcxx" ] && [ -f "$$src_libcxx" ]; then \
			cp -f "$$src_libcxx" "$$dst/"; \
		else \
			echo "    ⚠️  libc++_shared.so not found for $$ndk_triple under $(NDK_PREBUILT)"; \
		fi; \
	done
	@echo "✅ jniLibs refreshed for $(BUILD_TYPE)"