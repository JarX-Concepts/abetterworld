.PHONY: build-web run-web

build-web:
	cargo build --target wasm32-unknown-unknown --manifest-path apps/web/Cargo.toml
	wasm-bindgen target/wasm32-unknown-unknown/debug/web.wasm --out-dir apps/web/pkg --target web

start-web:
	$(MAKE) build-web
	cd apps/web && python3 -m http.server 8080

test-web:
	cd abetterworld && wasm-pack test --firefox --headless

.PHONY: build-ios-debug build-ios-release

CRATE_IOS = abetterworld_ios

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
	cbindgen apps/ios -o target/headers/abetterworld_ios.h
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
		-library target/aarch64-apple-ios/$(BUILD_DIR)/lib$(CRATE_IOS).a -headers apps/ios/src \
		-library target/universal/$(BUILD_DIR)/lib$(CRATE_IOS)_sim.a -headers apps/ios/src \
		-output target/xcframework/$(BUILD_DIR)/$(CRATE_IOS).xcframework

	@echo "✅ XCFramework created: target/xcframework/$(BUILD_DIR)/$(CRATE_IOS).xcframework"

.PHONY: build-android-debug build-android-release

ANDROID_TARGETS = \
	aarch64-linux-android \
	armv7-linux-androideabi \
	x86_64-linux-android

CRATE_ANDROID = abetterworld_android

build-android-debug:
	$(MAKE) build-android BUILD_TYPE=debug
	$(MAKE) build-android-cbindgen B

build-android-release:
	$(MAKE) build-android BUILD_TYPE=release

build-android-cbindgen:
	@echo "🔨 Building android header with cbindgen..."
	@command -v cbindgen >/dev/null 2>&1 || cargo install --locked cbindgen
	mkdir -p target/headers

	# Generate a single header we’ll attach to all slices
	cbindgen apps/android -o target/headers/abetterworld_android.h
	@echo "✅ Header generated: target/headers/abetterworld_android.h"


build-android:
	@echo "📱 Building $(BUILD_TYPE) for Android targets..."

	@for TARGET in $(ANDROID_TARGETS); do \
		echo "🔨 Building for $$TARGET..."; \
		cargo ndk -t $$TARGET -o target/android/$(BUILD_TYPE)/ $$CARGO_FLAGS build --package $(CRATE_ANDROID); \
	done

	@echo "✅ Android builds complete."	