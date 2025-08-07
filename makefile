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

build-ios-release:
	$(MAKE) build-ios-xcframework BUILD_TYPE=release

ifeq ($(BUILD_TYPE),release)
CARGO_FLAGS := --release
BUILD_DIR := release
else
CARGO_FLAGS :=
BUILD_DIR := debug
endif

build-ios-xcframework:
	@echo "🔨 Building $(BUILD_TYPE) for iOS + Simulator (unified xcframework)..."

	cargo build --package $(CRATE_IOS) --target aarch64-apple-ios $(CARGO_FLAGS)
	cargo build --package $(CRATE_IOS) --target x86_64-apple-ios $(CARGO_FLAGS)
	cargo build --package $(CRATE_IOS) --target aarch64-apple-ios-sim $(CARGO_FLAGS)

	mkdir -p target/universal/$(BUILD_TYPE)

	lipo -create \
		target/aarch64-apple-ios-sim/$(BUILD_TYPE)/libabetterworld_ios.a \
		target/x86_64-apple-ios/$(BUILD_TYPE)/libabetterworld_ios.a \
		-output target/universal/$(BUILD_TYPE)/libabetterworld_ios_sim.a

	@echo "📦 Creating xcframework..."
	mkdir -p target/xcframework/$(BUILD_TYPE)

	xcodebuild -create-xcframework \
		-library target/aarch64-apple-ios/$(BUILD_TYPE)/libabetterworld_ios.a \
		-headers apps/ios/src \
		-library target/universal/$(BUILD_TYPE)/libabetterworld_ios_sim.a \
		-headers apps/ios/src \
		-output target/xcframework/$(BUILD_TYPE)/abetterworld_ios.xcframework

	@echo "✅ XCFramework created: target/xcframework/$(BUILD_TYPE)/$(CRATE_IOS).xcframework"


.PHONY: build-android-debug build-android-release

ANDROID_TARGETS = \
	aarch64-linux-android \
	armv7-linux-androideabi \
	x86_64-linux-android

CRATE_ANDROID = abetterworld_android

build-android-debug:
	$(MAKE) build-android BUILD_TYPE=debug

build-android-release:
	$(MAKE) build-android BUILD_TYPE=release

build-android:
	@echo "📱 Building $(BUILD_TYPE) for Android targets..."

	@for TARGET in $(ANDROID_TARGETS); do \
		echo "🔨 Building for $$TARGET..."; \
		cargo ndk -t $$TARGET -o target/android/$(BUILD_TYPE)/ $$CARGO_FLAGS build --package $(CRATE_ANDROID); \
	done

	@echo "✅ Android builds complete."	