#!/usr/bin/env bash
# Packages the CLI: release build for the host (or the targets named on the
# command line), one .tar.gz plus a .sha256 per target, in dist/.
#
#   scripts/release.sh
#   scripts/release.sh aarch64-apple-darwin x86_64-apple-darwin
#
# Models are not bundled; the binary downloads them on first run.
set -euo pipefail

root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
cd "$root"
. "$root/scripts/dev-env.sh"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
version="${version:-0.0.0}"
host="$(rustc -vV | sed -n 's/^host: //p')"
targets=("$@")
[ "${#targets[@]}" -gt 0 ] || targets=("$host")
out="$root/dist"
mkdir -p "$out"

for target in "${targets[@]}"; do
  echo "== $target"
  cargo build --release -p openflow-cli --target "$target"
  src="$root/target/$target/release/openflow"
  [ -x "$src" ] || { echo "no binary at $src" >&2; exit 1; }

  stage="$(mktemp -d)"
  install -m755 "$src" "$stage/openflow"
  install -m644 "$root/LICENSE" "$stage/LICENSE"
  install -m644 "$root/README.md" "$stage/README.md"

  archive="$out/openflow-$version-$target.tar.gz"
  if [ -e "$archive" ] && [ "${FORCE:-0}" != "1" ]; then
    echo "refusing to overwrite $archive (FORCE=1 to replace)" >&2
    exit 1
  fi
  tar -czf "$archive" -C "$stage" openflow LICENSE README.md
  rm -rf "$stage"
  ( cd "$out" && sha256sum "$(basename "$archive")" > "$(basename "$archive").sha256" )
  echo "wrote $archive"
done

echo "verify with: cd dist && sha256sum -c ./*.sha256"
