#!/usr/bin/env bash
# Builds Nexdrop and collects the outputs in release/<version>/{build,installer}.
# Run from Git Bash (the MSVC environment is needed for the native dependencies).
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(python -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
# The updater signing key stays on this computer (git-ignored); without it no update can be published.
if [ ! -f .env.updater.key ] || [ ! -f .env.updater ]; then
  echo "Missing .env.updater.key / .env.updater (the update signing key)" >&2
  exit 1
fi
export TAURI_SIGNING_PRIVATE_KEY="$(cat .env.updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(sed -n 's/^TAURI_SIGNING_PRIVATE_KEY_PASSWORD=//p' .env.updater)"
pnpm tauri build
dest="release/$version"
rm -rf "$dest"
mkdir -p "$dest/build" "$dest/installer"
cp src-tauri/target/release/nexdrop.exe "$dest/build/Nexdrop.exe"
cp src-tauri/target/release/bundle/nsis/*"_${version}_"*-setup.exe "$dest/installer/"
cp src-tauri/target/release/bundle/nsis/*"_${version}_"*-setup.exe.sig "$dest/installer/"
python scripts/make_latest_json.py "$version" "$dest/installer"
python scripts/package_extension.py "$dest/extension"
echo "Release $version ready in $dest"
