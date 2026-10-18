#!/usr/bin/env bash
# Builds Nexdrop and collects the outputs in release/<version>/{build,installer}.
# Run from Git Bash (the MSVC environment is needed for the native dependencies).
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(python -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
pnpm tauri build
dest="release/$version"
rm -rf "$dest"
mkdir -p "$dest/build" "$dest/installer"
cp src-tauri/target/release/nexdrop.exe "$dest/build/Nexdrop.exe"
cp src-tauri/target/release/bundle/nsis/*"_${version}_"*-setup.exe "$dest/installer/"
echo "Release $version ready in $dest"
