.PHONY: build-web run-web

build-web:
	cargo build --target wasm32-unknown-unknown --manifest-path apps/web/Cargo.toml
	wasm-bindgen target/wasm32-unknown-unknown/debug/web.wasm --out-dir apps/web/pkg --target web

run-web:
	cd apps/web && python3 -m http.server 8080