#!/usr/bin/env bash
# Copies the foliate-js files the reader loads (no tests, build files or source maps) to $1.
set -euo pipefail
source="$(cd "$(dirname "$0")/../vendor/foliate-js" && pwd)"
dest="$1"
rm -rf "$dest"
mkdir -p "$dest"
for file in "$source"/*.js; do
    case "$(basename "$file")" in
        eslint.config.js | rollup.config.js | reader.js) ;;
        *) cp "$file" "$dest/" ;;
    esac
done
cp "$source/LICENSE" "$dest/"
(cd "$source" && find ui vendor -type f ! -name '*.map' -exec cp --parents {} "$dest/" \;)
