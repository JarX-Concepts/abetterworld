.PHONY: build-web run-web

build-web:
	cargo build --target wasm32-unknown-unknown --manifest-path apps/web/Cargo.toml
	wasm-bindgen target/wasm32-unknown-unknown/debug/web.wasm --out-dir apps/web/pkg --target web

run-web:
	cd apps/web && python3 -m http.server 8080

test-web:
	cd abetterworld && wasm-pack test --firefox --headless

.PHONY: build-ios-debug build-ios-release

CRATE = abetterworld_ios

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

	cargo build --package $(CRATE) --target aarch64-apple-ios $(CARGO_FLAGS)
	cargo build --package $(CRATE) --target x86_64-apple-ios $(CARGO_FLAGS)
	cargo build --package $(CRATE) --target aarch64-apple-ios-sim $(CARGO_FLAGS)

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

	@echo "✅ XCFramework created: target/xcframework/$(BUILD_TYPE)/$(CRATE).xcframework"