# A Better World 🌍

An experimental globe renderer and planetary visualization engine built in Rust using WGPU.

This is a personal research project exploring real-time planetary visualization, tile streaming, and cross-platform rendering. It’s still very early — expect rough edges, missing features, and plenty of TODOs — but the goal is to eventually support smooth, interactive navigation of Earth (and beyond) on desktop, web, and mobile.

If you’re curious, the screenshots and demos can give you a glimpse of where it’s heading.

---

## 🖼️ Screenshots

![World view](assets/platforms.png)
_Designed to work on all major platforms._

<div align="center">
  <img src="assets/nyc-wasm-1.png" alt="NYC WASM" width="45%"/>
  <img src="assets/nyc-wasm-2.png" alt="NYC WASM" width="45%"/>
</div>

![Tracy Integration](assets/abw-Tracy-iOS.png)
_Tracy Integration._

---

## 🚀 Getting Started

Clone the repository and build the project in release or debug mode:

```bash
git clone https://github.com/JarX-Concepts/abetterworld
cd abetterworld

# Build library
cargo build -p abetterworld
cargo build -p abetterworld --release

cp ./template.env .env
# Update .env file

# Run sample desktop app (mac, windows, linux)
cargo run -p desktop

# Run sample web app
cp ./abw_wasm_config_template.json abw_wasm_config.json
# Update abw_wasm_config.json file
make build-web
make run-web

# Run sample ios app
make build-ios-debug
# open examples/ios/AbetterWorld.xcodeproj in Xcode

# Run sample android app
make build-android-debug
# open examples/android in Android Studio

# Run Unit Tests on Desktop
cargo test -p abetterworld

# Run Unit Tests on Web
make test-web

```

---

## 📄 License

MIT — see [LICENSE](LICENSE) for details.

---

## 💬 Feedback & Contributions

This project is still in its early days.
If you stumble across it, feel free to peek under the hood, file issues, or share thoughts — but please don’t expect stability yet. Contributions are welcome once things settle a bit more!
