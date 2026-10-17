# Releasing

Nothing in this repo is pushed or published automatically. Releases are built and published by a
human, from a machine that has the toolchain for the target.

## What can be shipped today

The repo currently ships a **CLI** (`openflow`) plus the core, engines and platform crates. There is
no GUI app bundle: the Tauri shell the plan describes does not exist yet, and it cannot be compiled
on the headless Linux box this was built on (no webkit2gtk, no display).

```bash
scripts/release.sh                 # release build for the host + .tar.gz + .sha256 in dist/
scripts/release.sh x86_64-apple-darwin aarch64-apple-darwin   # cross-target, needs the toolchain
```

The script copies only the binary and the licence into the archive, writes a `.sha256`, and refuses
to overwrite an existing archive unless `FORCE=1` is set.

## Signing and notarisation (macOS)

The plan asked for a signed and notarised DMG. That needs a paid Apple Developer ID and an Apple
account, so it is **not** done here; the honest options, in order:

1. **Ad-hoc sign, ship with instructions.** Works on any Mac without an account:
   ```bash
   codesign --force --deep --sign - openflow                 # ad-hoc, no identity needed
   codesign -dv --verbose=4 openflow                         # show what was applied
   spctl -a -vv openflow                                     # Gatekeeper will still reject it
   ```
   Users then do one of: right-click → Open, or `xattr -d com.apple.quarantine <path>`. Say this
   plainly on the download page; do not imply it is notarised.
2. **Notarise, once someone has a Developer ID.** Requires an Apple account and money, so it needs
   the owner's yes:
   ```bash
   codesign --force --options runtime --timestamp --sign "Developer ID Application: <name> (<TEAM>)" openflow
   xcrun notarytool submit openflow-darwin.tar.gz --keychain-profile <profile> --wait
   xcrun stapler staple openflow
   ```
   Hardened runtime is required for notarisation and is what makes Gatekeeper stop warning.

When the Tauri shell lands upstream of this, the same two paths apply to the `.app`/DMG, with
`dmgbuild` or `create-dmg` for the container. The Linux and Windows builds are unsigned by
convention; Windows SmartScreen will warn until a code-signing certificate is bought.

## CI

`.github/workflows/ci.yml` runs on GitHub Actions (nothing runs it from this box — the repo is not
pushed anywhere). It builds and tests on Ubuntu, macOS and Windows, runs clippy with `-D warnings`
on Linux, and compile-gates the cross targets. Model-backed tests are `#[ignore]`d and need
`OPENFLOW_MODELS`, so CI stays fast and offline; run them locally with
`cargo test --workspace -- --include-ignored --test-threads=1`.

## Checklist before tagging

1. `cargo clippy --workspace --all-targets` — clean.
2. `cargo test --workspace` — green.
3. `cargo test --workspace -- --include-ignored --test-threads=1` on a machine with models.
4. `cargo check -p openflow-platform --target aarch64-apple-darwin` and
   `--target x86_64-pc-windows-gnu` — green.
5. `scripts/release.sh`, then `sha256sum -c dist/*.sha256` on the artifact.
6. Update `PROGRESS.md` and the version in `Cargo.toml`.
