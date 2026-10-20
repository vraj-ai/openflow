# OpenFlow on macOS (build from source)

Status: macOS arm64 compiles in CI but has not been run on real Mac hardware yet. Please report issues.

1. Install Xcode command line tools and Rust:
   ```bash
   xcode-select --install
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```
2. Build (Apple Silicon; add `--features metal` for GPU LLM/Whisper):
   ```bash
   cd openflow
   cargo build --release -p openflow-cli
   ./target/release/openflow --help
   ```
3. ONNX Runtime (Parakeet + Silero VAD): download the macOS arm64 release of ONNX Runtime from
   https://github.com/microsoft/onnxruntime/releases, unpack it, and
   `export ORT_DYLIB_PATH=/path/to/onnxruntime-osx-arm64-<ver>/lib/libonnxruntime.dylib`.
4. Models (downloaded from huggingface.co, SHA-checked):
   ```bash
   export OPENFLOW_MODELS=$HOME/openflow-models
   ./target/release/openflow models download parakeet-tdt-0.6b-v3
   ./target/release/openflow models download silero-vad
   ./target/release/openflow models download qwen3-4b-instruct-2507
   ./target/release/openflow doctor
   ```
5. Permissions (System Settings > Privacy & Security), granted to your terminal app
   (or to the binary if you run it directly):
   - **Microphone** — recording.
   - **Accessibility** — global hotkey and pasting into other apps.
   - **Input Monitoring** — may be requested for the key listener.
   Restart the terminal after granting. `openflow doctor` reports what is missing.
6. Run: `./target/release/openflow daemon`, hold the dictate key, speak, release.
   If the binary is quarantined: `xattr -d com.apple.quarantine ./openflow`.
