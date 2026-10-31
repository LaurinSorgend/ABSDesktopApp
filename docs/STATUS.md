# Status

Last updated 2026-10-30.

## Phase 0: done on Windows, Linux open

Windows parts of Phase 0 are finished and checked by hand. Results are in `docs/spike-findings.md`, the stack decision draft in `docs/decisions/0002-stack.md` (status: proposed until Linux numbers exist).

Open before Phase 0 exit criteria are met:

- Linux (GNOME and KDE on Wayland): system libmpv, MPRIS via souvlaki, `abs-reader` through `build_gtk`, fractional scaling in Slint, memory as PSS from `/proc/<pid>/smaps_rollup`. Needs a Linux machine.
- LGPL libmpv build for Windows releases. The dev build (`scripts/fetch-libmpv.ps1`, shinchiro 20261021) is GPL and 116 MB.
- CI and the release workflow run on GitHub; v0.1.0 was released on 2026-10-25.
- Long single-file M4B costs about 110 MB extra in libmpv/FFmpeg (MP4 sample index). symphonia 0.5.5 needs only 13 MB there but its AAC decoder fails on the fixture; see `docs/spike-findings.md`. Revisit after dogfooding.

## Facts Phase 1 builds on

- Auth routes are at the server root: `POST /login` with `x-return-tokens: true` returns `user.accessToken` and `user.refreshToken`; `POST /auth/refresh` with `x-refresh-token`. 2.37.2 rotates the refresh token on refresh, 2.26.3 returns the same one. Store whatever comes back. 2.37.2 still accepts the old token for 10 minutes (`REFRESH_TOKEN_GRACE_PERIOD`) and answers with the already rotated one; after that it answers 401.
- File downloads honour `Range` (206 with `Content-Range`) on 2.26.3 and 2.37.2. A range starting at the end of the file answers 500, not 416, so a complete `.part` file is renamed without a request.
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
- A paused server session checks `GET /api/me/progress/:id` on an interval and right after a realtime progress event. The interval is the setting `remote_check_seconds` (Off, 10 s, 30 s, 1 min, 5 min; default 30 s), changed in Settings through `Command::SetRemoteCheck`, which also applies it at once; Off turns the realtime trigger off too. When the server's `lastUpdate` is after this session's last sync and the position is at least 1 s away, the engine sends `RemoteProgress` and the app seeks the paused player there, so another device's listening carries over. Test: `paused_session_sees_another_device_move_on`, both servers.
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
- App icon: the Audiobookshelf logo (`packaging/icons/audiobookshelf-logo.png`, from ABS `client/static/Logo.png`); `packaging/make_icon.py` makes the PNG and ICO. It is the window icon, and `build.rs` embeds the ICO in the Windows exe through `winresource`. The tray icon is still the drawn play triangle in the accent color. Release builds use the Windows GUI subsystem (no console).
- The Windows zip ships the GPL shinchiro libmpv; that is allowed under our GPL-3.0 license, but the 116 MB size stays a reason for our own build.

CI runs on GitHub (`LaurinSorgend/ABSDesktopApp`, public since 2026-10-27). The first runs found Linux-only clippy lints, a fresh server answering item counts without `total`, and Ubuntu's older libmpv lacking some options we turn off; all fixed. The release workflow has not run yet (needs a `v*` tag).

1.11 dogfooding is with Laurin for a week. Phase 2 started alongside it.

Phase 2, 2.1 to 2.3 are implemented:
- Tables `download` (one row per item, with its folder) and `download_file` (one row per file, in play order, with `bytes_done`). The plan's single `download_job` table became these two.
- `abs_core::downloads`: `plan` turns the expanded item into files and a manifest; files are fetched to `<ino>.<ext>.part` with `Range` resume, size-checked and renamed. The manifest is written as `manifest.pending.json` and renamed to `manifest.json` when every file is there, so a manifest means complete. `cover.webp` is fetched at the end, best effort.
- Engine: `Download`, `PauseDownload`, `ResumeDownload`, `DeleteDownload`; events `DownloadsChanged`, `DownloadProgress` (at most twice a second). At most 2 items at once and 1 per server, files in order. Transient errors retry 5 times (2, 4, 8, 16 s), then the download is failed and can be retried. At start, running downloads go back in the queue and finished ones with missing or short files are fetched again. Removing an account deletes its downloads.
- Download folder: the `download_root` setting, default `<data dir>/downloads`. Each download stores its own folder, so changing the setting only affects new downloads. 200 MB stay free beyond the download size.
- A finished download plays from its files. With a connection the server session still runs, so listening time counts; without one, progress is only stored locally (offline session sync is 2.5).
- UI: download, pause, resume, cancel and remove on the item page; a download badge on tiles, a "Downloaded" library filter and a "Downloaded" row on Home (current library, newest 12 by when they were queued); a Downloads section in Settings with the folder (native folder dialog via `rfd`), space used and the list.
- Tests: `download_file_resumes_with_range` (abs-api) and `download_pause_resume_restart_and_delete` (engine, also checks local playback) pass on both servers. The UI has not been tried by hand yet.

2.4 is implemented:
- Connecting tries the server's addresses in order; the session keeps the one that answered.
- Every minute the engine pings connected servers (`GET /ping`, 10 s timeout). A failed ping, or a network error during sync or a request, marks the account offline and stops its socket; the next tick reconnects through the address list. When an address earlier in the list answers again (back on the home network), the engine reconnects through it.
- `Command::SetServerUrls` replaces the list and reconnects when the account is offline or its current address is gone. Settings shows the active account's addresses with add, remove and move up.
- Test: `server_addresses_fall_back_and_reconnect` (dead address only goes offline, dead then real reconnects, a restart skips the dead one).

2.5 and 2.6 are implemented:
- Table `local_session` (position columns, since `current_time` is an SQLite keyword). Playing without a server session records one with id `play_local_<uuid>`; so does a server session whose server stops answering mid-play (the tracker's unsent time moves over, the account goes offline).
- `AbsClient` reads the server's clock from each `Date` header (`clock_offset_ms`); `startedAt`/`updatedAt` are moved to the server clock before sending.
- `abs_core::offline::flush` sends an account's sessions with `POST /api/session/local-all`, leaves out the one still recording, and removes those the server took. Results come back in order but with the server's own ids (it swaps `play_local_` ids for UUIDs and keeps the mapping only in memory), so they are matched by position, and each session is sent once.
- Conflict rule: when the server's progress changed after the local session started and is more than the threshold away (setting `conflict_minutes`, default 5), the item's sessions wait and the UI asks. "Keep" sends `updatedAt` after the server's last update, "use the other" sends it before, so the server keeps that position; listening time counts either way. The prompt sits above the banner.
- Flushes run after each sync, every minute and when a local playback closes.
- Tests: unit tests for the clock offset, the conflict rule and the decision timestamps; `contract_offline` (download, go offline, play, reconnect, check server progress; then a phone moves the position, the conflict is asked and "keep" wins), on both servers. The test takes the server away by switching the account to a dead address instead of stopping the container, so the other contract tests keep running. New test user `traveller`, created by `docker/init_server.py`.
- Found on the way: older libmpv (Debian 12, Ubuntu 24.04) starts the first playlist entry when leaving idle, ignoring `playlist-pos`; the player now selects the wanted track again on `FileLoaded`. Reproduced and checked in a `rust:1.92-bookworm` container.

Phase 2 is implemented; its exit criterion (download, play offline, sync without lost or regressed progress) is what `contract_offline` checks. Not tried by hand in the UI yet.

## Phase 3

Phase 3 is done on Windows: Laurin tried EPUB, PDF and CBZ in the reader by hand, including the round trip with the web reader. The contract tests pass on both servers.

- What the web reader stores (2.26.3 and 2.37.2 alike): EPUB writes the epub.js start CFI of the page (a point CFI) and its fraction, and only reads back values starting with `epubcfi`. PDF and comics write the 1-based page as a number and `(page - 1) / pages`. MOBI saves nothing. The server keeps `ebookLocation` as a string column but may hand a number back, so `abs-api` accepts both.
- ADR 0004: PDF (pdf.js 5.5.207, vendored by foliate-js) and CBZ open in `abs-reader` through foliate-js. No pdfium, no comic view in Slint.
- `crates/reader-ipc` (`abs-reader-ipc`) has the JSON-lines messages. The reader starts hidden, waits for `open` on stdin, and reuses its window for the next book. `relocate` has a collapsed (point) CFI for reflowable books, or `page`/`pages` for fixed layout. Settings changed in the reader window come back as `settings`; the app stores them under `reader_settings` (one set for all books).
- The reader page has a top bar (contents, title, chapter, settings), the TOC from foliate's `ui/tree.js`, a settings panel (pages/scroll, light/sepia/dark, book/serif/sans font, size, line height, margins), arrow/PageUp/PageDown/space keys, T and S for the panels, and wheel paging in paginated mode. WebView2 goes to the low memory level while minimised.
- `abs-reader <file> [location]` still opens a book directly for testing.
- Core: `ebooks` finds the book in a finished download, else fetches it through `/api/items/:id/ebook/:ino` (no download permission needed) into `<cache>/<account>/ebooks/` (newest 10 kept), else uses a cached copy when offline. `Command::OpenEbook` answers with `EbookReady`/`EbookFailed`.
- `Command::SaveEbookProgress` stores the position locally and puts it in `pending_sync` before sending, so a quit mid-request loses nothing. Queued positions are sent (newest per item) at the start of every offline flush; only a 4xx refusal drops them; network errors, 5xx answers and expired logins leave them queued. No conflict prompt for reading positions: last write wins.
- Downloads include the ebook file (`manifest.ebook`); ebook-only items can be downloaded.
- App: the item page shows Read when the item has an EPUB, PDF, CBZ, MOBI, AZW3 or FB2, and "N % read". The app debounces `relocate` for 5 s and saves on close, on switching books and on quit. The reader is `abs-reader` next to the app exe, or `ABS_READER_PATH`; in development run `cargo build -p abs-reader` once so it exists in `target/debug`.
- Packaging: both scripts build `abs-reader` and copy foliate-js without tests, build files and source maps (5.6 MB, `scripts/copy-foliate.*`). `THIRD-PARTY-LICENSES.html` now covers the whole workspace; license texts of foliate-js, pdf.js, zip.js and fflate are in `packaging/licenses`. The Windows script builds and packs; the Linux one has not run.
- Tests: unit tests for IPC messages, positions in web reader format, the cache and queue payloads, ebook download plans. Contract tests `ebook_file_and_reading_position` (abs-api) and `contract_ebooks` (engine: open, save, offline queue, reconnect; new test user `reader`, so rerun `docker/init_server.py` on existing servers).
- Found by hand: the Alice PDF scan was blank because pdf.js 5.5 needs its wasm decoders for JPEG 2000 and JBIG2, which foliate-js does not vendor. `abs-reader` now embeds them from pdfjs-dist 5.5.207 (`web/pdfjs-wasm`) and passes `wasmUrl`.
- Package size (Windows zip): 59.5 MB before the reader, 62.5 MB after (foliate-js 5.1 MB and `abs-reader.exe` 1.5 MB unpacked). libmpv is 115 MB of the 154 MB unpacked; a slim LGPL build is planned in PLAN.md.

Linux under WSL (Ubuntu 24.04, libmpv 0.37, WebKitGTK 2.52, WSLg's Weston on Wayland), 2026-10-25:
- clippy, unit tests and the full integration suite (both servers, libmpv playback, ebook contracts) pass.
- Laurin ran the app under WSLg: audio playback and the reader (EPUB, PDF, CBZ through `build_gtk`) work. WSLg is not GNOME or KDE, so MPRIS, fractional scaling and the desktop-specific checks stay open.
- The app panics at start without `libxkbcommon-x11-0`: winit loads `libxkbcommon-x11.so` at runtime, and `libxkbcommon-dev` does not pull it in. linuxdeploy will not see a library that is only loaded at runtime, so the AppImage has to add it by hand or list it as a host dependency.
- Without a D-Bus session bus (as in WSL), credentials fall back to files as designed, but souvlaki's MPRIS thread panics on an `unwrap()` inside the crate. The app keeps running without media controls. The `MediaControls` setup should catch this (check the bus first, or run souvlaki behind `catch_unwind`) so a missing bus is a logged warning.

Open:
- Linux: the two fixes above and the Hyper-V VM tests (GNOME and KDE on Wayland, AppImage) are in PLAN.md under "Linux fixes and VM test".
- Measure the reader memory again (Phase 0 budget: reader tree 110 MB) and check it is freed when the window closes.

## Phase 4

Phase 4 is implemented. Its exit criterion (two servers with overlapping books give one entry per work, no wrong merges on the fixture) is what `contract_works` checks. Laurin has not tried the UI by hand yet; it was clicked through once under Windows (all-accounts page, a work page, a split).

- 4.1: every connected account runs its own minute tick (token refresh, ping, offline flush, sync), so a slow server no longer holds up the others. Startup logins to the same server are 1 s apart. Accounts have an `enabled` column (migration 0004); `Command::SetAccountEnabled` drops the session and socket but keeps the login, and requeues the account's running downloads. Adding an existing account again turns it back on. Settings has "Turn off"/"Turn on" per account.
- 4.3: `abs_core::works` (`normalize`, `matching`, `source`). Tables `work`, `work_member`, `work_override` (migration 0005). Overrides point at an item (`target_account_id`, `target_item_id`) instead of the plan's `target_work_id`, because work ids are not stable enough to point at. A rebuild matches everything again (about 1000 items in well under a second) and keeps a work's id when most of its members stay; the engine runs it 500 ms after catalog syncs, realtime catalog changes, account on/off, removal and override commands, then sends `WorksChanged`.
- Matching: only book libraries of enabled accounts. Cascade: same server and item id, equal ASIN, equal ISBN-13 (ISBN-10 converted, check digit verified), normalised title (NFKC, lowercase, punctuation, leading article via `titleIgnorePrefix` or "the/a/an") + primary author + duration within 2 % or 5 min. Items without a duration only match each other. Laurin's call: different ASINs, ISBNs, narrators, `abridged` or ebook formats keep the ISBN and title steps apart (also through other members of a work), so two narrations are two works.
- 4.4: work page with the copies (account, library, narrator, length, progress, why they match), "Not the same book" (split), "Undo" for overrides, and "Add a copy" searching all accounts to merge by hand. The item page has a copies button.
- 4.2: an "All accounts" page (sidebar entry when there is more than one account): one tile per book or every copy, chips to pick the libraries, sort and filter. Mode and hidden libraries are stored in settings. The per-account view is unchanged. Tiles of mixed grids are named `account/item`.
- 4.5: `works::playback_source` picks the copy with the latest progress, else a download, else the work's preferred account, else the general one (setting `preferred_account`), else the oldest account; copies that can play now (connected or downloaded) go first. Progress goes only to the account that played. Setting `mirror_progress` (off by default) copies the position with `PATCH /api/me/progress/:id` to copies on other accounts whose length is within 30 s or 0.5 %, on pause, forced syncs and close; offline accounts are skipped, not queued.
- 4.6: `fixtures/media/matching` (short silent MP3s) as library "Matching"; `docker/init_server.py` patches metadata per server version (table in `fixtures/README.md`). Rerun `python fixtures/fetch_fixtures.py` and `docker/init_server.py` on existing servers. `contract_works` asserts zero wrong merges and allows one known miss: 2.26.3 cuts "The Happy Prince and Other Tales" short, so its title cannot match. It also checks mirroring. `contract_accounts` covers two servers at once and on/off across a restart.
- Not done: the cover of a merged tile is the oldest account's copy; the hover card and author link on a mixed grid use that copy's account. Progress on merged tiles updates live only for the shown copy until the page is opened again.

## Themes

Implemented, not yet tried by hand in every theme.

- Themes are JSON files in the private repository `LaurinSorgend/abs-desktop-themes` (format in its README): metadata (name, version, creator, description, website, source, donation), a widget set (`catppuccin`, `audiobookshelf`, `material`, `winui`, `glass`), a look (fonts, radii, pill shapes, borders, focus ring, control height, shadows, floating panels, content layer, backdrop) and palettes with accents. Only the widget sets are code (`ui/*.slint`, `src/theme/styles.rs`).
- `build.rs` bundles `themes/*.json` of that repository: from `ABS_THEMES_DIR` when set (CI checks the repository out with the `THEMES_TOKEN` secret, which needs read access to it), otherwise it clones or updates `<target>/<profile>/abs-desktop-themes` over SSH. Offline it uses the last clone with a warning. The fetch runs whenever the build script runs, not on every build; the packaging scripts set `ABS_THEMES_FETCH` so a release always fetches.
- Installed themes go to `<data dir>/themes/<id>.json`; one with the id of a bundled theme replaces it. Settings store the theme by id and palettes and accents by name per theme (`light_palette.<id>`, `dark_palette.<id>`, `accent.<id>`); the old Catppuccin flavor and accent indices are still read.
- `theme.json` now holds local changes only (colors per mode, fonts, text size, radius, density, border width, focus ring). Export writes the shown palettes with those changes as a new theme file.
- User choices on top of any theme: font and number font (bundled plus system fonts via `fontdb`), text size, radius, density, borders, focus ring.
- WinUI on Windows 11 puts Mica behind the window (`src/backdrop.rs`, DWM system backdrop with a transparent Slint window); every style sets the title bar to dark or light, and the non-Mica ones color it like the window. Glass shows the playing item's cover blurred behind its panels (a 96 px copy made on the cover decoder thread).
- Tests: every bundled theme parses, survives writing and reading again, and has text on base at 7:1 or better in every palette and accent.
- Themes can also place things and pick single widgets: a `layout` block (navigation left, right or top; player bar at the bottom or top, or a panel on the left or right) and a `widgets` block (navigation items, text fields, segmented buttons, slider thumbs, cover hover, shelves, play button, sheen, control edge, button weight). Each style sets the defaults, so older theme files look as before. `src/theme/layout.rs`.

## Player views

Implemented, not yet tried by hand.

- One window in three shapes (`src/app/views.rs`, `ui/pages/views.slint`). Slint globals belong to one window, so a second window would need all player state twice; instead the main window turns into the mini player and back. Its pages are rebuilt when it returns.
- Mini player: frameless, 440 x 184 by default, dragged by its top strip (`WindowMoveArea`). Cover, a display with the time in the number font and accent, title, chapter and time left, the book bar with chapter markers, compact transport, volume and speed (a button that steps through the speeds, since a list does not fit the window). A pin keeps it on top (setting `mini_on_top`); its position and size are kept in `mini_geometry`, also when the app quits in it. Going back restores the main window's place, size and maximized state.
- Full screen: the playing cover blurred (the copy the Glass style already uses) under a gradient of the theme's base color, so text keeps the theme's colors. Large cover with the theme's cover radius and shadow, chapter, title in the heading weight, author, the book bar with chapter markers (and the chapter bar when the progress setting asks for it), speed and sleep timer, a 64 px play button and volume.
- F11 toggles full screen, Esc leaves it; the player keys work in all three views. The player bar, the player page and the player panel have buttons for both views; in a narrow window the bar drops volume to make room for them.

## Local folders

Implemented; the reasons are in ADR 0006. The first-run page and Settings can add a folder, and the app works with no server.

- `abs_core::local`: `group` (files to books), `names` (folder names), `order` (track order, chapters), `probe` (tags, chapters), `metadata` (precedence, OPF, EPUB, sidecars), `book` (the stored item) and `scan`. The engine side is `engine/local.rs`.
- Tests: unit tests in those modules, and `contract_local`, which uses `fixtures/media` directly and checks the folder stays unchanged. Only its last test needs a server.
- Tried in the real app on Windows, seeded with the fixture folders: Home, library, item page, M4B playback with a chapter jump, the Settings list and the Authors view.
- Not tried in the real app: the native folder dialog (the tests send `AddLocalFolder`), the reader window, and an MP3 book playing across a file boundary. One pause click in that session seemed not to register; the next one worked. It was a synthetic window message and has not been looked into.
## Switching accounts

The active account in the sidebar or top bar is a button (`ui/accounts.slint`). It opens a list of all accounts with their state and "Add account"; picking one does what "Use" in Settings does, including going to Home. Arrow keys and Enter work in the list. Tried in the real app with two accounts; `screens` also renders the list.

## Failure handling

Fixed after a code review on 2026-10-30. `contract_recovery` covers most of it, with a proxy in front of the server that can answer 502.

- A token refresh runs in its own task, so a cancelled caller cannot lose a rotated token. Reconnecting an account within a run shares the previous client's tokens (`AbsClient::with_auth_of`), so running playback and downloads keep working.
- The HTTP client fails a request after 60 s of silence on an open connection.
- Local progress is stamped on the server's clock, using the last offset seen for the account. An account that has been offline since the app started has no offset and uses the local clock. Taking the server's position in a conflict overwrites the local row.
- Two tests need extra setup and are skipped otherwise: `stalled_server_times_out` (`ABS_TEST_PAUSE_CONTAINER`, pauses the server, run it alone) and `old_session_survives_a_reconnect` (`ABS_TEST_SHORT_GRACE_SERVER`, a 2.37 server with `REFRESH_TOKEN_GRACE_PERIOD=2`).

## Phase 5

- 5.1: OIDC moved to the cross-cutting section "Single sign-on" in `PLAN.md`, because ABS rejects allowed redirect URIs with a port. Instead the add-account page has "Password" and "API key"; `Command::AddAccountWithKey`, `account::add_account_with_key`, migration 0006 (`account.api_key`). Restoring an API key account calls `login_api_key` with the stored key. Before 2.37.0 such accounts start no socket. `contract_accounts::api_key_account_survives_restart` covers both servers. The page has not been looked at by hand yet.
- 5.2 and 5.4: Velopack (ADR 0005). `crates/app/src/updates.rs` is compiled in only with the `self-update` feature on Windows; Settings then has an Updates section (look at start, on by default; "Look for updates"; "Restart now" once downloaded). A downloaded update is also applied on the next start. `scripts/package-velopack.ps1` builds Setup.exe, Portable.zip and the update packages; `scripts/package-windows.ps1` without `-SelfUpdate` builds the zip for winget and Scoop. `ABS_UPDATE_FEED=<vpk output folder>` replaces GitHub for tests. Tried locally: portable 0.1.0 downloaded 0.1.1 from a feed and applied it on the next start. Not tried: "Restart now" and the installer.
- 5.2 and 5.3: `scripts/package-deb.sh` (FHS layout in `/usr/lib/abs-desktop`, a link in `/usr/bin`, Depends from `dpkg-shlibdeps` plus `libxkbcommon-x11-0`), `scripts/package-source.sh` and `scripts/render-manifests.py` (PKGBUILD, Scoop, winget from `packaging/*/*.in`). The release workflow runs them, keeps the manifests as the `manifests` artifact (not in the release) and adds the Velopack files to the draft with `vpk upload github --merge`. Checked: the `.deb` installs and starts on Ubuntu 24.04 (WSL); the PKGBUILD builds in an `archlinux` container (needs `options=('!lto')`, else the bundled SQLite does not link); `winget validate` passes. Scoop: the bucket `LaurinSorgend/scoop-bucket` (Excavator updates it every four hours); `scoop install laurin/abs-desktop` of 0.2.0 works. Not checked: installing through winget.
- `build.rs` now runs ssh with `BatchMode=yes`; before, a fetch that needed a host key or passphrase hung the build forever.
- 5.5: README, CONTRIBUTING, CHANGELOG. The screenshots in `docs/screenshots` are the real app (WinUI 3, dark) against a temporary server with a demo library of public-domain books, silent audio and generated covers.
- Open, for Laurin: apply to SignPath Foundation, decide whether the themes may be published in the source tarball (the themes repository has no license file yet), submit to winget and the AUR, and hand-write the Flathub manifest.
