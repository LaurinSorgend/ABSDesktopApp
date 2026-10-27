#!/usr/bin/env bash
# Packs dist/abs-desktop-<version>-source.tar.gz: the tracked files of HEAD, foliate-js and the
# themes, so the app builds without git or access to the themes repository (AUR, Flathub, distros).
# Set ABS_THEMES_DIR to a checkout of the themes repository.
set -euo pipefail
version="${1:-dev}"
version="${version#v}"
root="$(cd "$(dirname "$0")/.." && pwd)"
themes="${ABS_THEMES_DIR:?set ABS_THEMES_DIR to a checkout of the themes repository}"
name="abs-desktop-$version"
work="$root/dist/source"
stage="$work/$name"

rm -rf "$work"
mkdir -p "$stage/abs-desktop-themes"
git -C "$root" archive HEAD | tar -x -C "$stage"
git -C "$root/vendor/foliate-js" archive HEAD | tar -x -C "$stage/vendor/foliate-js"
cp -r "$themes/themes" "$stage/abs-desktop-themes/"
cp "$themes/README.md" "$stage/abs-desktop-themes/" 2>/dev/null || true

tar -C "$work" -czf "$root/dist/$name-source.tar.gz" "$name"
echo "packed dist/$name-source.tar.gz"
