#!/usr/bin/env bash
# Builds the release binaries and packs them with libmpv and foliate-js into dist/abs-desktop-<version>-x86_64.AppImage.
# Needs linuxdeploy on PATH (or LINUXDEPLOY set) and cargo-about (cargo install cargo-about --features cli).
set -euo pipefail
version="${1:-dev}"
root="$(cd "$(dirname "$0")/.." && pwd)"
appdir="$root/dist/AppDir"
docs="$appdir/usr/share/doc/abs-desktop"

cargo build --release --locked -p abs-desktop -p abs-reader

rm -rf "$appdir"
mkdir -p "$docs/licenses" "$appdir/usr/bin"
"$root/scripts/copy-foliate.sh" "$appdir/usr/bin/foliate-js"
cp "$root/LICENSE" "$root/packaging/README.txt" "$docs/"
cp "$root"/packaging/licenses/* "$root"/crates/app/ui/fonts/licenses/* "$docs/licenses/"
cargo about generate --locked --workspace -c "$root/packaging/about.toml" -m "$root/Cargo.toml" \
    "$root/packaging/about.hbs" -o "$docs/THIRD-PARTY-LICENSES.html"

cd "$root/dist"
LDAI_OUTPUT="abs-desktop-$version-x86_64.AppImage" "${LINUXDEPLOY:-linuxdeploy}" \
    --appdir "$appdir" \
    --executable "$root/target/release/abs-desktop" \
    --executable "$root/target/release/abs-reader" \
    --desktop-file "$root/packaging/linux/abs-desktop.desktop" \
    --icon-file "$root/packaging/icons/abs-desktop.png" \
    --output appimage
echo "packed dist/abs-desktop-$version-x86_64.AppImage"
