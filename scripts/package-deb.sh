#!/usr/bin/env bash
# Builds dist/abs-desktop_<version>_amd64.deb: the binaries and foliate-js in /usr/lib/abs-desktop,
# a link in /usr/bin, the desktop file and the icon. Uses the system libmpv; no self-update.
# Needs dpkg-dev (dpkg-shlibdeps) and cargo-about (cargo install cargo-about --features cli).
set -euo pipefail
version="${1:-0.0.0}"
version="${version#v}"
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$root/dist/deb"
pkg="$work/abs-desktop"
lib="$pkg/usr/lib/abs-desktop"
docs="$pkg/usr/share/doc/abs-desktop"
# winit loads it at runtime, so dpkg-shlibdeps does not see it.
runtime_depends="libxkbcommon-x11-0"

# A new value makes the build fetch the latest themes.
export ABS_THEMES_FETCH="$(date +%s)"
cargo build --release --locked -p abs-desktop -p abs-reader

rm -rf "$work"
mkdir -p "$lib" "$pkg/usr/bin" "$pkg/DEBIAN" "$docs/licenses" \
    "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$root/target/release/abs-desktop" "$root/target/release/abs-reader" "$lib/"
strip --strip-unneeded "$lib/abs-desktop" "$lib/abs-reader"
"$root/scripts/copy-foliate.sh" "$lib/foliate-js"
ln -s ../lib/abs-desktop/abs-desktop "$pkg/usr/bin/abs-desktop"
install -m 644 "$root/packaging/linux/abs-desktop.desktop" "$pkg/usr/share/applications/"
install -m 644 "$root/packaging/icons/abs-desktop.png" "$pkg/usr/share/icons/hicolor/256x256/apps/"
install -m 644 "$root/LICENSE" "$docs/copyright"
install -m 644 "$root/packaging/README.txt" "$docs/"
install -m 644 "$root"/packaging/licenses/* "$root"/crates/app/ui/fonts/licenses/* "$docs/licenses/"
cargo about generate --locked --workspace -c "$root/packaging/about.toml" -m "$root/Cargo.toml" \
    "$root/packaging/about.hbs" -o "$docs/THIRD-PARTY-LICENSES.html"

mkdir -p "$work/debian"
printf 'Source: abs-desktop\n\nPackage: abs-desktop\nArchitecture: any\n' > "$work/debian/control"
shlibs="$(cd "$work" && dpkg-shlibdeps -O -e"$lib/abs-desktop" -e"$lib/abs-reader" 2>/dev/null)"
depends="${shlibs#shlibs:Depends=}, $runtime_depends"

cat > "$pkg/DEBIAN/control" <<CONTROL
Package: abs-desktop
Version: $version
Architecture: amd64
Maintainer: Laurin Sorgend <laurin@sorgend.eu>
Installed-Size: $(du -sk "$pkg/usr" | cut -f1)
Depends: $depends
Section: sound
Priority: optional
Homepage: https://github.com/LaurinSorgend/ABSDesktopApp
Description: Desktop client for Audiobookshelf
 Streams and downloads audiobooks and podcasts from one or more
 Audiobookshelf servers, keeps progress in sync and reads ebooks,
 PDFs and comics.
CONTROL

deb="$root/dist/abs-desktop_${version}_amd64.deb"
dpkg-deb --root-owner-group --build "$pkg" "$deb"
echo "packed $deb"
