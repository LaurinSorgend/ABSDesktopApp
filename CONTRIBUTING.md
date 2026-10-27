# Contributing

## Setup

You need Rust 1.92 or later, Python 3, Docker and git with submodules:

```
git clone --recurse-submodules https://github.com/LaurinSorgend/ABSDesktopApp.git
```

The build bundles the themes from the private repository `LaurinSorgend/abs-desktop-themes`. With SSH access, `build.rs` clones it into `target/<profile>/abs-desktop-themes` and updates it later. Without access, set `ABS_THEMES_DIR` to a folder that has a `themes` subfolder, such as the `abs-desktop-themes` folder in a release's source tarball.

### Windows

Fetch libmpv into `vendor/libmpv/x64`. The script needs `gh`, `7z` and the MSVC tools:

```
powershell -File scripts/fetch-libmpv.ps1
```

The player's build script copies `libmpv-2.dll` next to the built exes. `scripts/run.ps1` builds and starts the app (`-Release` for an optimized build, `-DataDir <folder>` to keep data out of your normal profile).

### Linux (Debian, Ubuntu)

```
sudo apt install libmpv-dev libwebkit2gtk-4.1-dev libgtk-3-dev libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-0
```

Then `cargo run -p abs-desktop`.

## Test servers

Two Audiobookshelf servers run in Docker: the oldest supported version (2.26.3) on port 13378 and the newest tested version (2.37.2) on port 13379. The fixtures are public-domain audiobooks and ebooks.

```
python fixtures/fetch_fixtures.py
docker compose -f docker/docker-compose.yml up -d
python docker/init_server.py http://localhost:13378
python docker/init_server.py http://localhost:13379
```

`init_server.py` creates the users, libraries, a collection and a series, and waits for the scans. It is safe to run again. Logins are `admin` / `admin-pass` and `listener` / `listener-pass`. The other users belong to single tests.

## Tests

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --workspace --features abs-api/integration,abs-core/integration,abs-player/integration
```

The last line runs the contract tests against both Docker servers, and the player tests play the fixtures through libmpv with no audio output. CI runs all of these.

`ABS_DESKTOP_DATA_DIR=<folder>` keeps the database, covers, downloads and logins (as files, not in the keyring) in one folder.

## Packaging

| Script | Output |
|---|---|
| `scripts/package-windows.ps1 -Version <v>` | Zip without self-update, for winget and Scoop |
| `scripts/package-velopack.ps1 -Version <v>` | Velopack installer, portable zip and update packages in `dist/velopack` (needs `vpk`, see ADR 0005) |
| `scripts/package-linux.sh <v>` | AppImage (needs linuxdeploy) |
| `scripts/package-deb.sh <v>` | `.deb` (needs dpkg-dev) |
| `scripts/package-source.sh <v>` | Source tarball with foliate-js and the themes (needs `ABS_THEMES_DIR`) |
| `python scripts/render-manifests.py <v> dist` | PKGBUILD, Scoop and winget manifests in `dist/manifests`, with the hashes of the files in `dist` |

The scripts that build the app need cargo-about (`cargo install cargo-about --features cli`). Pushing a `v*` tag runs all of them in `.github/workflows/release.yml` and leaves a draft release. The manifests are not part of the release; the run keeps them as the `manifests` artifact. The [Scoop bucket](https://github.com/LaurinSorgend/scoop-bucket) updates itself from new releases.

To try an update without a release, pack two versions with `vpk pack`, start the older portable build with `ABS_UPDATE_FEED` set to the newer one's output folder, and restart it after it reports the download.

## Conventions

- Keep functions under 50 to 100 lines; split them when they grow.
- Public items get short `///` doc comments. Prefer clear names over explanatory comments.
- All user-facing text goes through `@tr()` in Slint. Text built in Rust comes from functions on the `Strings` global.
- `core` never depends on Slint. `abs-reader` talks to the app only over its IPC protocol (`crates/reader-ipc`).
- Small commits with one-sentence messages.
- Architecture decisions go into `docs/decisions/NNNN-title.md`.
