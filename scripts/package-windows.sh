#!/usr/bin/env bash
set -euo pipefail

version=${1:?usage: package-windows.sh <version>}
[[ $version =~ ^[0-9A-Za-z.+-]+$ ]] || { echo "invalid version: $version" >&2; exit 2; }

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

target=x86_64-pc-windows-gnu
release_dir="target/$target/release"
zip_path="$repo_root/dist/open-g-hub-$version-windows-x86_64.zip"
setup_path="$repo_root/dist/open-g-hub-$version-windows-x86_64-setup.exe"

command -v makensis >/dev/null || { echo "makensis not found; install NSIS" >&2; exit 2; }

cargo build --release --locked --target "$target" -p ogh-cli -p ogh-daemon -p ogh-gui

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp "$release_dir/ogh.exe" "$release_dir/ogh-daemon.exe" "$release_dir/ogh-gui.exe" \
  "$release_dir/ogh-tray.exe" packaging/windows/README.txt "$stage/"

mkdir -p dist
rm -f "$zip_path" "$setup_path"
zip -j -q "$zip_path" "$stage/ogh.exe" "$stage/ogh-daemon.exe" "$stage/ogh-gui.exe" \
  "$stage/ogh-tray.exe" "$stage/README.txt"
makensis -V2 -DVERSION="$version" -DSTAGE="$stage" -DOUTFILE="$setup_path" \
  packaging/windows/installer.nsi
echo "$zip_path"
echo "$setup_path"
