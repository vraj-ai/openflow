# Source this from anywhere inside the repo or one of its worktrees: `. scripts/dev-env.sh`.
# Keeps cargo's cache in the repo and points native builds at the locally
# extracted dev files in vendor/sysroot (nothing is installed system-wide).
OPENFLOW_ROOT="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
export CARGO_HOME="$OPENFLOW_ROOT/.cargo-home"
export ORT_DYLIB_PATH="$OPENFLOW_ROOT/vendor/onnxruntime/onnxruntime-linux-x64-1.31.0/lib/libonnxruntime.so"
export OPENFLOW_MODELS="$OPENFLOW_ROOT/models"
if [ -d "$OPENFLOW_ROOT/vendor/sysroot" ]; then
  export PKG_CONFIG_SYSROOT_DIR="$OPENFLOW_ROOT/vendor/sysroot"
  export PKG_CONFIG_PATH="$OPENFLOW_ROOT/vendor/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$OPENFLOW_ROOT/vendor/sysroot/usr/share/pkgconfig"
fi
