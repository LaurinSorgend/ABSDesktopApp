# Status

Last updated 2026-10-25.

## Phase 0: done on Windows, Linux open

Windows parts of Phase 0 are finished and checked by hand. Results are in `docs/spike-findings.md`, the stack decision draft in `docs/decisions/0002-stack.md` (status: proposed until Linux numbers exist).

Open before Phase 0 exit criteria are met:

- Linux (GNOME and KDE on Wayland): system libmpv, MPRIS via souvlaki, `abs-reader` through `build_gtk`, fractional scaling in Slint, memory as PSS from `/proc/<pid>/smaps_rollup`. Needs a Linux machine.
- LGPL libmpv build for Windows releases. The dev build (`scripts/fetch-libmpv.ps1`, shinchiro 20261021) is GPL and 116 MB.
- CI and the release workflow have never run: there is no GitHub remote yet.
- Long single-file M4B costs about 110 MB extra in libmpv/FFmpeg (MP4 sample index). symphonia 0.5.5 needs only 13 MB there but its AAC decoder fails on the fixture; see `docs/spike-findings.md`. Revisit after dogfooding.

## Facts Phase 1 builds on

- Auth routes are at the server root: `POST /login` with `x-return-tokens: true` returns `user.accessToken` and `user.refreshToken`; `POST /auth/refresh` with `x-refresh-token`. 2.37.2 rotates the refresh token on refresh, 2.26.3 returns the same one. Store whatever comes back.
- File downloads honour `Range` (206 with `Content-Range`) on 2.26.3 and 2.37.2.
- `POST /api/items/:id/play` with `deviceInfo`, `mediaPlayer`, `supportedMimeTypes` returns `audioTracks` with relative `contentUrl`, `startOffset`, `duration`.
- libmpv needs `demuxer-max-bytes`/`demuxer-max-back-bytes` capped and `media-controls=no`. Track switches emit `END_FILE` reason `STOP` (2), not an error.
- Access tokens live 12 h on 2.26.3 and 1 h on 2.37.2; read `exp`, never assume. 2.26.3 tokens have no `jti`, so two logins of one user in the same second get identical tokens, and logging out one ends both.
- API keys (`POST /api/api-keys`) work as bearer tokens on both versions and have no `exp`.
- `GET /api/me/progress` is 404 on 2.26.3 and works on 2.37.2. `abs-api` gates it at 2.36.0 (from the plan, not verified against release notes).
- The auth rate limiter is configured only by env (`RATE_LIMIT_AUTH_MAX`, `0` turns it off); the test servers turn it off.
- Workspace MSRV is 1.92 (Slint 1.18). reqwest 0.13 uses the `rustls` feature (aws-lc-rs).

## Dev setup

```
python fixtures/fetch_fixtures.py
docker compose -f docker/docker-compose.yml up -d
python docker/init_server.py http://localhost:13378   # ABS 2.26.3
python docker/init_server.py http://localhost:13379   # ABS 2.37.2
powershell -File scripts/fetch-libmpv.ps1             # Windows only
```

Logins: `admin` / `admin-pass`, `listener` / `listener-pass`.

Spikes: `cargo run --release` with `-p abs-api --example smoke`, `-p abs-desktop --example cover_grid` (`SPIKE_AUTOSCROLL=1`), `-p abs-player --example mpv_spike`, `-p abs-player --example mpv_idle`, `-p abs-desktop --example media_controls` (plays audio), `-p abs-reader -- <book> [cfi]`.

## Phase 1

1.1 and 1.2 are done. `abs-api` has `AbsClient` (single-flight refresh, one retry on 401, `TokenSink` for refresh tokens), typed models with `Raw<T>` for raw JSON, and the MVP endpoints. Tests:

```
cargo test -p abs-api                          # unit tests + decode of fixtures/api/<version>/
cargo test -p abs-api --features integration   # contract tests against both Docker servers
python fixtures/capture_api.py <url>           # refresh the JSON fixtures
```

1.3 is done: `abs_core::db` (rusqlite 0.40 bundled, WAL, foreign keys, `rusqlite_migration`). `Db::store()` gives the repositories (accounts, catalog, progress, queue, settings), `Db::write` runs them in one transaction. Item search uses an FTS5 external-content table kept in sync by triggers. The progress column is `position`, because `current_time` is an SQLite keyword. Item rows also have `added_at`, which the plan's schema lacked.

1.4 is done:
- `abs_core::credentials::Credentials` keeps refresh tokens in the OS keyring (`keyring-core` with the Windows store, or the zbus Secret Service store on Linux), keyed by account UUID. `Credentials::system()` returns `Unavailable` when the keyring does not answer; the UI then offers `files(dir)` (0600) or `memory()`. `sink(account)` plugs into `AbsClient::with_token_sink`. The real-keyring test is `#[ignore]` (`cargo test -p abs-core -- --ignored`); it passed on Windows. The Linux store has not been compiled yet.
- `abs_core::cover_cache::CoverCache` stores `<root>/<account>/covers/<item>_<width>_<etag>.webp`. The etag (the item's `cover_etag`) is in the file name, unlike the plan's path, so a changed cover is a cache miss. Byte-capped LRU, default 500 MB, order kept across restarts through file mtimes.

1.5 is done:
- `abs_core::account`: `add_account` (status probe, login, server/URL/account rows, token saved) and `restore_account` (first reachable URL, stored refresh token).
- `abs_core::sync`: `sync_catalog` pages each library (500 per page) and removes items that are gone; `sync_progress` uses `/api/me/progress` or the `/api/me` payload and replaces bookmarks.
- `abs_core::engine::Engine`: a current-thread tokio runtime on the `abs-engine` thread. `Command`s in, `Event`s out through a callback (the app forwards them with `slint::invoke_from_event_loop`). Every minute it refreshes due tokens, retries offline accounts and re-syncs when `sync_interval_minutes` (default 30) has passed. Accounts that need a login wait for the user.
- The database is one connection behind a `Mutex`; writes run inline on the engine thread. Fine for now, revisit if a large library sync stalls the engine.
- Contract tests: `cargo test -p abs-core --features integration`.

1.6 is done:
- `abs_api::Realtime` is an own Engine.IO v4 client over `tokio-tungstenite` (ADR 0003). It authenticates with the access token (or API key), re-sends `auth` whenever `AbsClient::token_changes` fires, answers pings, and reconnects with backoff from 1 s to 60 s. It stops after logout or an `auth_failed` that a refresh cannot fix.
- Events used: `init`, `auth_failed`, `user_item_progress_updated`, `item_added`/`item_updated`/`items_added`/`items_updated`, `item_removed`, `library_*`, `user_session_closed` (names checked in `SocketAuthority.js` of 2.26.3 and 2.37.2).
- The engine runs one realtime task per connected account. Progress events update the `progress` table, item events update the catalog (unknown library triggers a catalog sync), and a reconnect pulls progress.

1.7 is done (`abs-player`, no dependency on `abs-api`):
- `Player` trait and `MpvPlayer`: tracks as an mpv playlist, global time mapped with `timeline::locate`, cross-track seeks applied on `FileLoaded`, position events every 250 ms, `Ended` after the last track. Demuxer cache capped at 8/2 MiB, mpv's own scripts and `media-controls` off.
- `controller::Controller`: skip back/forward (10/30 s default), next/previous chapter (restart if more than 3 s in), speed, volume, sleep timer (fixed time or end of chapter) with a 10 s fade.
- `media_controls`: `MediaControls` trait with a souvlaki implementation and `MediaKey` mapping.
- Tests: unit tests, plus `cargo test -p abs-player --features integration` playing the fixtures through libmpv with `ao=null`.

1.8 is done:
- `abs_core::playback`: `start` (device id of the account, full MIME list, no forced direct play so HLS comes only when the server picks it), `ActivePlayback::sync` and `close`. A 404 on sync or close starts a new session and repeats the call. Local progress is written on every sync.
- `ListenTracker` counts real listening time (at most 5 s per report, so a laptop sleep is not counted) and says when to sync: every 15 s while playing and at once on pause. The app forces a sync on seek and chapter change.
- Engine: `StartPlayback`, `ReportPlayback`, `StopPlayback` run in order on their own task; events `PlaybackStarted`, `PlaybackFailed`, `PlaybackHeaders` (new `Authorization` after a refresh). The open session is closed when the engine shuts down.
- Server restart (2.37.2, `docker restart`): sync answers 404, the new session takes the position. ABS also closes a user's open session when the same device id starts another one.

1.9 is implemented; the hands-on pass is open:
- `abs-desktop` is a Slint app (`crates/app/ui/*.slint`, compiled by `build.rs`). All UI text is `@tr()`; texts Rust builds come from the `Strings` global's functions, because Slint has no Rust-side `tr!`.
- The UI thread opens its own read connection to the database (WAL) and shows local data at once. Network data comes from engine requests (`LoadCover`, `LoadItem`, `LoadShelves`, `SearchServer`, `LoadGroups`, `LoadAuthorItems`, `SetFinished`, bookmarks). Cover bytes are decoded on a separate thread.
- Theme: Catppuccin flavor for light and dark, accent, mode, plus `theme.json` in the data folder (`{"light": {"accent": "#d20f39"}, "dark": {...}}`; color names in `src/theme.rs`). Fonts from the design kit (Ronzino, Fira Code) are bundled as TTF with their OFL texts.
- Tray (close to tray while playing) is Windows only; on Linux `tray-icon` needs a GTK main loop.
- `ABS_DESKTOP_DATA_DIR=<dir>` puts database, covers and logins (as files, not the keyring) in one folder, for testing.
- Found on a real server: home shelves such as "continue series" send `media.metadata.series` as one object. `abs-api` accepts both, and shelves skip entities they cannot decode.
- Not done: no UI automation tests; theme swatches do not refresh when the system switches light/dark while the settings page is open.

1.10 is written, not yet run on GitHub (no remote):
- `ci.yml`: `check` (fmt, clippy, unit tests) on Windows and Linux; `integration` on Linux fetches the fixtures (cached by the script's hash), starts both Docker servers and runs `cargo test --workspace --features abs-api/integration,abs-core/integration,abs-player/integration`. That command passes locally against both servers.
- `docker/init_server.py` now waits until the scans finish, so one run sets up a fresh server.
- `release.yml` on `v*` tags: `scripts/package-windows.ps1` (zip with exe, `libmpv-2.dll`, LICENSE, README, `THIRD-PARTY-LICENSES.html` from cargo-about, mpv/FFmpeg/font licenses) and `scripts/package-linux.sh` (AppImage via linuxdeploy on Ubuntu 22.04), then a draft GitHub release. The Windows zip builds and starts locally (61 MB, most of it libmpv); the Linux script has not run yet.
- cargo-about config is `packaging/about.toml`; it needs `cargo install cargo-about --features cli`.
- App icon: `packaging/icons/` from `packaging/make_icon.py`, used as the window icon. The exe has no embedded icon yet. Release builds use the Windows GUI subsystem (no console).
- The Windows zip ships the GPL shinchiro libmpv; that is allowed under our GPL-3.0 license, but the 116 MB size stays a reason for our own build.

CI runs on GitHub (private repo `LaurinSorgend/ABSDesktopApp`). The first runs found Linux-only clippy lints, a fresh server answering item counts without `total`, and Ubuntu's older libmpv lacking some options we turn off; all fixed. The release workflow has not run yet (needs a `v*` tag).

1.11 dogfooding is with Laurin for a week. Phase 2 started alongside it.

Phase 2, 2.1 to 2.3 are implemented:
- Tables `download` (one row per item, with its folder) and `download_file` (one row per file, in play order, with `bytes_done`). The plan's single `download_job` table became these two.
- `abs_core::downloads`: `plan` turns the expanded item into files and a manifest; files are fetched to `<ino>.<ext>.part` with `Range` resume, size-checked and renamed. The manifest is written as `manifest.pending.json` and renamed to `manifest.json` when every file is there, so a manifest means complete. `cover.webp` is fetched at the end, best effort.
- Engine: `Download`, `PauseDownload`, `ResumeDownload`, `DeleteDownload`; events `DownloadsChanged`, `DownloadProgress` (at most twice a second). At most 2 items at once and 1 per server, files in order. Transient errors retry 5 times (2, 4, 8, 16 s), then the download is failed and can be retried. At start, running downloads go back in the queue and finished ones with missing or short files are fetched again. Removing an account deletes its downloads.
- Download folder: the `download_root` setting, default `<data dir>/downloads`. Each download stores its own folder, so changing the setting only affects new downloads. 200 MB stay free beyond the download size.
- A finished download plays from its files. With a connection the server session still runs, so listening time counts; without one, progress is only stored locally (offline session sync is 2.5).
- UI: download, pause, resume, cancel and remove on the item page; a download badge on tiles and a "Downloaded" library filter; a Downloads section in Settings with the folder (native folder dialog via `rfd`), space used and the list.
- Tests: `download_file_resumes_with_range` (abs-api) and `download_pause_resume_restart_and_delete` (engine, also checks local playback) pass on both servers. The UI has not been tried by hand yet.

2.4 is implemented:
- Connecting tries the server's addresses in order; the session keeps the one that answered.
- Every minute the engine pings connected servers (`GET /ping`, 10 s timeout). A failed ping, or a network error during sync or a request, marks the account offline and stops its socket; the next tick reconnects through the address list. When an address earlier in the list answers again (back on the home network), the engine reconnects through it.
- `Command::SetServerUrls` replaces the list and reconnects when the account is offline or its current address is gone. Settings shows the active account's addresses with add, remove and move up.
- Test: `server_addresses_fall_back_and_reconnect` (dead address only goes offline, dead then real reconnects, a restart skips the dead one).

Next: 2.5 offline session sync, 2.6 the offline round-trip test.
