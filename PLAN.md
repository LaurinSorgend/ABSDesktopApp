# ABS desktop client: implementation plan (Rust)

Based on `reports/Audiobookshelf native desktop app plan.md`. Stack: Rust workspace, Slint UI, libmpv for audio, souvlaki for MPRIS/SMTC, a separate `abs-reader` process (tao + wry + foliate-js) for ebooks, PDF and comics (pdf.js through foliate-js, ADR 0004). Minimum supported server: Audiobookshelf 2.26.0. Latest at time of writing: 2.37.2.

Phases build on each other. Each phase ends with exit criteria; don't start the next phase until they pass.

## Conventions

- Rust stable, edition 2024, one Cargo workspace.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass in CI.
- Functions stay under 50-100 lines; split into smaller functions when they grow.
- Public items get `///` doc comments, kept short. Avoid explanatory inline comments; rename or extract instead.
- Small commits with one-sentence messages.
- Architecture decisions go into `docs/decisions/NNNN-title.md` (one short ADR each).
- All user-facing strings go through Slint's `@tr()` from the start, so translation later needs no refactor.
- Any JS tooling (only foliate-js related, if ever needed) uses pnpm.

## Workspace layout

```
ABSDesktopApp/
  Cargo.toml              workspace
  crates/
    abs-api/              HTTP client, serde models, token refresh, Socket.IO
    core/                 accounts, SQLite, sync, downloads, merge, cover cache
    player/               Player trait, libmpv backend, media controls, sleep timer
    app/                  Slint UI, view models, tray
    abs-reader/           separate binary: tao + wry + foliate-js
  vendor/foliate-js/      git submodule, pinned commit
  docker/                 docker-compose for test ABS servers
  fixtures/               test media (public domain), JSON fixtures
  docs/decisions/
```

Dependency direction: `app -> core -> abs-api`, `app -> player`, `core` never depends on Slint. `abs-reader` talks to `app` only over IPC.

---

## Phase 0: Spike (1-2 weeks)

Goal: prove the risky parts and measure memory before writing real code. Spike code may be thrown away.

### 0.1 Repository and tooling
- [x] Create the Cargo workspace with the five crates as empty skeletons.
- [x] Add `.gitignore` (target/, local DBs, `.claude/`, `CLAUDE.md`).
- [x] GitHub Actions workflow with a matrix of `windows-latest` and `ubuntu-latest`: build, clippy, test.
- [x] Choose the license (GPL-3.0 recommended: fits Slint's free license and libmpv). Record as ADR 0001.

### 0.2 Test server and fixtures
- [x] `docker/docker-compose.yml` with two Audiobookshelf services: one pinned to 2.26.x, one to the latest release. Verify the official image name and the first-run `/init` request before scripting it.
- [x] Script that initialises the server, creates an admin plus a second user, and creates a library.
- [x] Collect fixtures:
  - a multi-file MP3 book (LibriVox),
  - a 30-40 h M4B with chapters (concatenate public domain audio with ffmpeg),
  - an EPUB (Project Gutenberg), a PDF, a CBZ.
- [x] Note which files have ASIN/ISBN metadata; you'll need some with and some without for Phase 4.

### 0.3 API smoke test
- [x] Small binary in `abs-api/examples/` that:
  - calls `GET /status` and prints `serverVersion`,
  - logs in with `POST /login` and `x-return-tokens: true`,
  - refreshes with `POST /auth/refresh` and `x-refresh-token`,
  - lists libraries and the first page of items with `minified=1`,
  - downloads a cover with width and `format=webp`.
- [x] Test whether `GET /api/items/:id/file/:ino/download` honours a `Range` header (answers an open question from the report).

### 0.4 Slint cover grid
- [x] Slint window with a `ListView` of rows, column count derived from window width.
- [x] Load 500 items from the test server (duplicate the library if needed).
- [x] Decode covers on a worker thread, push `slint::Image` back via `invoke_from_event_loop`.
- [x] Byte-capped LRU for decoded images (start at 64 MB).
- [ ] Check scrolling smoothness, HiDPI on Windows, fractional scaling on Wayland.
  - Windows checked and fine; Wayland open.

### 0.5 libmpv playback
- [ ] Windows: get an LGPL libmpv build (`-Dgpl=false`, FFmpeg without `--enable-gpl`), check the DLL set and its size. Planned as its own build repository, see Slim LGPL libmpv.
- [ ] Linux: link against the system libmpv.
- [x] Use `libmpv2` to play a direct-play `contentUrl` with the `Authorization` header set via `http-header-fields`.
- [x] Multi-file book: load tracks as a playlist; map global time to `(track, offset)` using `startOffset`.
- [x] 40 h M4B: measure seek latency at 10 %, 50 %, 95 % over HTTP.
- [x] Speed 0.75x-3.0x: confirm pitch is preserved (scaletempo2).
- [x] Set `demuxer-max-bytes` and observe memory while streaming.

### 0.6 Media controls
- [x] souvlaki with the HWND from Slint's winit backend on Windows 11 (SMTC overlay, media keys).
- [ ] MPRIS on GNOME and KDE (Wayland), using the zbus backend.
- [x] Show title, author, cover, position.

### 0.7 Reader helper
- [x] Add foliate-js as a git submodule, pinned to a commit.
- [x] `abs-reader`: tao window + wry webview, serving foliate-js and the book through a custom protocol (no localhost server, no token in JS).
- [ ] Open the EPUB fixture on Windows 11, GNOME Wayland, KDE Wayland.
- [x] Print `relocate` events (CFI, fraction) to stdout.

### 0.8 Measurements and decision
- [ ] Measure on Windows (Private Working Set) and Linux (PSS from `/proc/<pid>/smaps_rollup`):
  - idle main window,
  - browsing 500 covers,
  - playing audio,
  - reader process with one EPUB open.
- [ ] Record numbers in `docs/decisions/0002-stack.md` with a go/no-go for Slint (fallback: Qt) and libmpv (fallback: symphonia).
- [ ] Set provisional memory budgets from the measurements (for example: idle, browsing, playing) and add them to this plan.
  - Windows (private WS), from `docs/decisions/0002-stack.md`: idle 90 MB, browsing 150 MB, playing +40 MB (+140 MB for a long single-file M4B), reader tree 110 MB. Linux pending.

**Exit criteria:** memory measured on both OSes; long-M4B seeking acceptable; media controls work on both OSes; reader opens on Wayland; stack decision written down.

---

## Phase 1: MVP

Goal: a good single-view player on a data model that already supports many accounts.

### 1.1 `abs-api`: client and auth
- [x] `AbsClient` per account: base URL, shared `reqwest::Client` (rustls), token state.
- [x] Models with `#[serde(default)]`, unknown fields ignored, and the raw JSON kept alongside typed fields.
- [x] Auth:
  - [x] `login(username, password)` with `x-return-tokens: true`.
  - [x] `login_api_key(key)` as an alternative.
  - [x] Parse the JWT `exp` claim; schedule refresh before expiry. Never hard-code lifetimes.
  - [x] Single in-flight refresh per account (mutex or `tokio::sync::OnceCell`-style guard).
  - [x] Persist the new refresh token before dropping the old one (rotation on every use).
  - [x] On 401: refresh once, retry once, then surface "login required".
  - [x] `logout()` via `POST /logout` with `x-refresh-token`.
- [x] Version gating: read `serverVersion` from `/status`, expose `supports(feature)`.
- [x] Respect `ROUTER_BASE_PATH` (server under a subpath).
- [x] Endpoints for the MVP:
  - `GET /api/me`, `GET /api/me/progress` (2.36+, else login payload `mediaProgress`)
  - `GET /api/libraries`, `GET /api/libraries/:id/items` (paginated, `minified=1`, base64 filters)
  - library-scoped series, authors, collections, search, personalized shelves
  - `GET /api/items/:id?expanded=1`
  - `POST /api/items/:id/play`, `POST /api/session/:id/sync`, `POST /api/session/:id/close`
  - bookmarks create/update/delete
- [x] Error type: network, auth, server (status + body), decode.

### 1.2 `abs-api`: contract tests
- [x] Integration tests behind a `--features integration` flag, run against both Docker servers.
- [x] One test per endpoint used: request succeeds, response decodes.
- [x] Token test: refresh rotation, expired access token recovery.

### 1.3 `core`: storage
- [x] `rusqlite` with WAL and foreign keys; migrations via `rusqlite_migration`.
- [x] Schema v1:
  - `server(id, name)`, `server_url(server_id, url, priority)`
  - `account(id uuid, server_id, user_id, username, device_id, created_at)`
  - `library(account_id, remote_id, name, media_type, raw_json)`
  - `item(account_id, remote_id, library_id, title, title_sort, author, narrator, series, asin, isbn, duration, cover_etag, updated_at, raw_json)`
  - `progress(account_id, item_id, current_time, progress, is_finished, ebook_location, ebook_progress, updated_at)`
  - `bookmark(account_id, item_id, time, title)`
  - `pending_sync(id, account_id, kind, payload, created_at)`
  - `setting(key, value)`
  - FTS5 table over title, author, narrator, series.
- [x] Repository layer with one module per table group; no SQL outside `core::db`.

### 1.4 `core`: credentials and cover cache
- [x] `keyring-core` with the Windows store and the zbus Secret Service store; entries keyed by account UUID.
- [x] Only the refresh token goes to the keyring; access tokens stay in memory.
- [x] No Secret Service on Linux: ask the user ("don't remember me" or a 0600 file), never silently store plaintext.
- [x] Cover cache: `<cache>/<account_id>/covers/<item_id>_<w>.webp`, size-capped LRU (setting, default 500 MB).

### 1.5 `core`: accounts and sync
- [x] Add-account flow: URL -> `/status` probe -> login -> store server, URL, account.
- [x] Generate a stable `device_id` per account.
- [x] Initial catalog sync: page through each library, upsert items, delete items no longer present.
- [x] Periodic re-sync (setting, default 30 min) because there is no "changed since" endpoint.
- [x] Progress sync from `/api/me/progress` on start and after reconnect.
- [x] `Command` / `Event` channels; tokio runtime on its own thread; Slint thread only receives events.

### 1.6 `abs-api`/`core`: realtime
- [x] Own Engine.IO client (ADR 0003, replaces `rust_socketio`): connect to server root, emit `auth`, wait for `init` / `auth_failed`.
- [x] Re-auth after every token refresh; reconnect with backoff.
- [x] Handle `user_item_progress_updated` (update `progress`, emit event) and item add/update/remove events (verify exact names in `SocketAuthority.js`).
- [x] Keep the socket behind one small type (`abs_api::Realtime`) so its implementation can be replaced.

### 1.7 `player`
- [x] `Player` trait: `load(tracks, start)`, `play`, `pause`, `seek(global_secs)`, `set_speed`, `set_volume`, event stream (position, state, ended, error).
- [x] `MpvPlayer`: playlist of tracks, global time <-> `(track, offset)`, demuxer cache cap.
- [x] Chapters from the server JSON (not from file parsing); next/previous chapter.
- [x] Skip back/forward with configurable intervals.
- [x] Sleep timer: fixed duration or end of chapter, with fade-out.
- [x] Media controls via souvlaki, behind its own trait (fallbacks: `windows` crate SMTC, `mpris-server`).

### 1.8 `core`: playback sessions
- [x] Start: `POST /api/items/:id/play` with `deviceInfo` (per-account `device_id`), `mediaPlayer`, and a full `supportedMimeTypes` list (mpeg, mp4, m4b, aac, flac, ogg, opus, webm, wav, ...).
- [x] Sync every 15 s while playing, and on pause, seek and chapter change (`currentTime`, `timeListened`).
- [x] Close on stop, item change and app exit.
- [x] Sync returns 404 (server restarted, sessions are in memory): start a new session at the current position.
- [x] Use HLS only if the server returns it; libmpv plays it as is.

### 1.9 `app`: UI
- [x] Add-account screen and account switcher (one active account in the MVP).
- [x] Home: continue listening, recently added, personalized shelves.
- [x] Library grid with sort and filter; series, authors, collections views.
- [x] Search (local FTS5 first, server search as fallback).
- [x] Item detail: description, chapters, progress, play, mark finished.
- [x] Player bar + full player view: cover, chapter list, speed, sleep timer, bookmarks.
- [x] Volume and mute; player bar progress for the book, the chapter or both; chapter markers on the book bar.
- [x] Settings: accounts, playback (skip intervals, default speed), cache size.
- [x] Themes: system/light/dark, Catppuccin flavor per mode, accent color, `theme.json` overrides.
- [x] Keyboard shortcuts (space, arrows, chapter jumps).
- [x] Optional tray icon (`tray-icon`) with play/pause; close-to-tray while playing. Windows only for now.
- [x] Error and offline banners; never a blank screen.
- [x] Hands-on pass of every screen against both test servers and a real library.

### 1.10 CI and builds
- [x] CI job with the Docker ABS services running the integration tests on Linux.
- [ ] Release workflow on tags: Windows zip (exe + libmpv DLLs + license files), Linux AppImage.
- [x] Include third-party license texts (libmpv, FFmpeg LGPL, Slint, crates via `cargo-about`).

### 1.11 Dogfooding
- [ ] Use the app for a week alongside the phone app.
- [ ] Check listening stats on the server are correct (session API, not direct progress patches).
- [ ] Leave it idle overnight and through laptop sleep; confirm token refresh and socket reconnect.

**Exit criteria:** listening on desktop and phone keeps progress and stats consistent; token refresh survives a day of idle and sleep; memory stays within the Phase 0 budgets.

---

## Phase 2: Downloads and offline

### 2.1 Download manager
- [x] `download_job` table: account, item, file `ino`, size, bytes done, state.
- [x] Per-file download from `/api/items/:id/file/:ino/download` (check `canDownload` first).
- [x] Write to `.part`, resume with `Range` (if 0.3 showed no Range support, restart the file instead), verify size, rename.
- [x] Concurrency: 2-3 files total, 1 per server.
- [x] Pause, resume, cancel, retry with backoff; survive app restart.
- [x] Free disk space check before starting.

### 2.2 Storage layout
- [x] `<root>/<account_id>/<item_id>/<ino>.<ext>` plus `manifest.json` (item JSON, chapters, track offsets) and `cover.webp`.
- [x] Configurable download root; storage usage screen; delete download.
- [x] Startup integrity check: manifest present, file sizes match.

### 2.3 Offline playback
- [x] Player picks local files when the download is complete; chapters and offsets come from the manifest.
- [x] Downloaded filter and badge in the library.

### 2.4 Connectivity and server URLs
- [x] Per-server URL list (LAN, VPN, public); probe `/status` in priority order, cache the working one.
- [x] Online/offline state per account, shown in the UI.

### 2.5 Offline session sync
- [x] Record local sessions with id `play_local_<uuid>` in SQLite while offline (start, current time, time listened, `updatedAt`).
- [x] Track the server clock offset from the HTTP `Date` header; correct `updatedAt` before sending.
- [x] On reconnect: flush with `POST /api/session/local-all`; handle each `{id, success, progressSynced}`; keep failed ones in the queue.
- [x] Conflict prompt when local and server positions differ by more than a threshold (setting, default 5 min): keep local, take server.

### 2.6 Tests
- [x] Integration test: download, stop the container, play offline, start the container, flush, check server progress.
- [x] Unit tests for clock offset correction and the conflict rule.

**Exit criteria:** a book downloaded, played offline and synced after reconnect without lost or regressed progress.

---

## Phase 3: Ebook reader

### 3.1 Research step
- [x] Read `EpubReader.vue`, `PdfReader.vue` and `ComicReader.vue` in the ABS client to confirm what each writes to `ebookLocation` and `ebookProgress`.
  - EPUB (epub.js): `ebookLocation` is the start CFI of the visible page (a point, not a range), `ebookProgress` its fraction; only CFIs starting with `epubcfi` are read back. PDF and comics: the 1-based page number and `(page - 1) / pages`. MOBI saves nothing. Same on 2.26.3 and 2.37.2.
- [x] Decide PDF rendering: native Slint view with pdfium-render, or PDF.js inside the reader. Record as an ADR.
  - PDF.js inside the reader, and CBZ through foliate-js too (ADR 0004).

### 3.2 `abs-reader` binary
- [x] tao + wry window; custom protocol serves foliate-js assets and the book bytes from a file path given by the main app.
- [x] Strict CSP; no network access from the webview.
- [x] On Linux, use `build_gtk` for Wayland. Works under WSLg; GNOME and KDE are in Linux fixes and VM test.
- [x] On Windows, lower WebView2 memory with `set_memory_usage_level` when the window is hidden or minimised.

### 3.3 IPC
- [x] JSON lines over stdin/stdout.
- [x] Main -> reader: `open {path, format, location, settings}`, `settings {...}`, `close`.
- [x] Reader -> main: `ready`, `relocate {cfi, fraction, chapter}`, `toc [...]`, `error`, `closed`.
  - `relocate` also carries `page` and `pages` for PDF and comics; `settings` goes back to main when the user changes them in the reader window.
- [x] Shared message types in a tiny crate or module used by both binaries (`crates/reader-ipc`).

### 3.4 Main app integration
- [x] Fetch the ebook from `/api/items/:id/ebook/:fileid?` into the cache (or use the downloaded file).
- [x] Launch the helper, pass the saved `ebookLocation`.
- [x] On `relocate`: debounce (about 5 s), then `PATCH /api/me/progress/:id` with `ebookLocation` (CFI) and `ebookProgress`; queue when offline.
- [x] Ebook downloads reuse the Phase 2 download manager.

### 3.5 Reader features
- [x] Paginated and scroll modes.
- [x] Font size, font family, line height, margins.
- [x] Light, dark, sepia themes.
- [x] Table of contents panel; keyboard and mouse-wheel navigation.
- [x] Remember settings per user, not per book.

### 3.6 PDF and CBZ
- [x] PDF via the approach chosen in 3.1 (pdf.js through foliate-js, ADR 0004; no pdfium).
- [x] CBZ through foliate-js as well (ADR 0004), instead of `zip` + `image` in the main app.
- [x] CBR: not supported (unrar license); MOBI/AZW3 work through foliate-js but are untested extras.

### 3.7 Round-trip test
- [x] Set a position in the desktop reader, open the ABS web reader: same chapter and page.
- [x] Reverse direction.

**Exit criteria:** EPUB positions round-trip with the web reader in both directions; reader memory is freed when the helper exits.

---

## Phase 4: All accounts at once and library merging

### 4.1 Simultaneous accounts
- [x] Per-account supervisor task owning its refresh loop, socket and periodic sync.
- [x] Stagger startup refreshes (auth is rate limited to 40 requests per IP per 10 min by default).
- [x] Account enable/disable without deleting data.

### 4.2 View modes
- [x] Per-account view (as in the MVP).
- [x] Filtered view: choose any set of accounts and libraries.
- [x] Merged view: one entry per work.
- [x] Remember the view per window/session.

### 4.3 Work layer
- [x] Tables: `work(id, title, author, cover_source)`, `work_member(work_id, account_id, item_id, match_method, confidence)`, `work_override(account_id, item_id, action, target_work_id)`.
- [x] Normalisation helpers: Unicode NFKC, lowercase, strip punctuation and leading articles (use `titleIgnorePrefix`), primary author normalisation, ISBN-10 -> ISBN-13.
- [x] Matching cascade, in order:
  1. same server and same item id under different users (exact),
  2. equal ASIN,
  3. equal ISBN-13,
  4. normalised title + primary author + duration within tolerance (start at 2 % or 5 min, whichever is larger).
- [x] Never match on title alone.
- [x] Re-run matching incrementally after each sync; overrides always win.

### 4.4 Manual control
- [x] Merge two works, split a member out; store in `work_override`.
- [x] Work detail shows every member with account, library, narrator, duration, progress.

### 4.5 Playback source and progress
- [x] Pick the source: member with the most recent progress, else a local download, else the preferred account (setting per work and global).
- [x] Progress is written only to the account that played.
- [x] Optional setting: mirror progress via `PATCH /api/me/progress` to members with near-identical duration.

### 4.6 Tests
- [x] Fixture: two ABS containers with overlapping libraries, including different narrators and an abridged edition.
- [x] Unit tests for every cascade step and the normalisation helpers.
- [x] Assert zero wrong merges on the fixture; log missed merges for tuning.

**Exit criteria:** two servers with overlapping books show one entry per work, with no wrong merges on the test fixture.

---

## Phase 5: SSO and distribution

### 5.1 OIDC login
- [ ] PKCE S256; loopback listener on `127.0.0.1` with a fixed port; open the system browser.
- [ ] Same cookie jar for `/auth/openid` and `/auth/openid/callback` (else `400 No session`).
- [ ] Clear error when the redirect URI isn't on the server's `authOpenIDMobileRedirectURIs` list; admin instructions in the docs.

### 5.2 Windows
- [ ] Installer and delta updates with Velopack (or cargo-packager NSIS/MSI); compare both briefly, record an ADR.
- [ ] Code signing through SignPath Foundation (needs a public repo and CI builds); apply early, approval takes time.
- [ ] Updates hosted on GitHub Releases.
- [ ] winget manifest; portable zip stays available.

### 5.3 Linux
- [ ] AppImage (keep from Phase 1), `.deb`, AUR PKGBUILD.
- [ ] Flathub:
  - hand-write the manifest (Flathub's requirements page bans AI-generated or AI-assisted manifests; re-read it before submitting),
  - vendored crates via `flatpak-cargo-generator`, no network during build,
  - GNOME runtime (provides WebKitGTK for the reader), libmpv as a module,
  - minimal permissions, Secret portal for credentials (oo7),
  - validated AppStream metainfo with screenshots.

### 5.4 Update policy
- [ ] Cargo feature `self-update`, enabled only for the Windows installer and portable builds.
- [ ] Package-manager builds (winget, Flatpak, deb, AUR) never self-update.

### 5.5 Docs and release
- [ ] README with screenshots, install instructions per channel, minimum server version.
- [ ] CONTRIBUTING with the dev setup (Docker ABS, fixtures, libmpv on Windows).
- [ ] Changelog generated from commits or kept by hand.

**Exit criteria:** installs and updates cleanly through each channel, with no two updaters competing.

---

## Cross-cutting work

### Player views

Not scheduled yet; do after dogfooding, together with the theming options.
- [ ] Mini player: a small separate window in the style of Winamp, always on top if the user wants. Cover, title, chapter, transport, progress bar, volume and speed. Switching to it hides the main window; a button switches back. Remembers its position.
- [ ] Full screen player in the style of Spotify's full screen view: large cover, title, author and chapter, transport, progress bar with chapter markers, speed and sleep timer. The background is the cover, blurred and darkened so the text stays readable.
- [ ] Slint has no blur filter: blur the cover in Rust (`image` crate) once per item, on the decoder thread, at a small size and scaled up.
- [ ] Keyboard: F11 or a button toggles full screen, Esc leaves it; the player shortcuts work in both views.

### Theming options

Beyond what 1.9 has (mode, flavor, accent, `theme.json` colors).
- [x] UI font and the font for numbers selectable (bundled faces plus installed system fonts), with a font size scale.
- [x] Corner radius as a theme value (0 by default), applied to buttons, fields, tiles, covers and popups.
- [x] Density: spacing and tile size (compact, normal, large).
- [x] Border width and focus ring style.
- [x] All of these in `theme.json` too, and shareable theme files (import/export).
- [x] Themes with their own widget set: Catppuccin, Audiobookshelf, Material, WinUI 3 (Mica, Windows type ramp, settings cards) and Glass (floating translucent panels, the playing cover blurred behind them).
- [x] Themes live in the private repository `LaurinSorgend/abs-desktop-themes` with name, version, creator, description, website, source and donation link; the build bundles all of them, users can install more.
- [ ] Try every theme by hand in light and dark, on Windows and Linux.

### Linux fixes and VM test

Found running the app under WSL (Ubuntu 24.04, WSLg), where audio and the reader work. WSLg is Weston, not GNOME or KDE, so the desktop checks need a VM.

- [ ] Media controls without a D-Bus session bus: souvlaki's MPRIS thread panics on an `unwrap()` inside the crate. Check for the session bus before creating `MediaControls` (or run it behind `catch_unwind`), log a warning and carry on without media keys.
- [ ] `libxkbcommon-x11.so` is loaded by winit at runtime; without `libxkbcommon-x11-0` the app panics at start. Bundle it in the AppImage by hand (linuxdeploy only sees linked libraries), add it to the `.deb` dependencies and the CI package list, and note it in the README for building from source.
- [ ] Hyper-V test VMs: Ubuntu 24.04 (GNOME) from Hyper-V Quick Create and a Kubuntu or Fedora KDE VM from an ISO. Use the basic session, not the enhanced session, which runs X11 over xrdp. The test servers are reached through the PC's LAN address, not `localhost`.
- [ ] In both VMs, on Wayland: install the AppImage from a release build (`scripts/package-linux.sh`), log in, play, and use the reader with EPUB, PDF and CBZ.
- [ ] In both VMs: MPRIS shows title, author, cover and position in the desktop's media widget, and play/pause, seek and next/previous work from it (closes the Phase 0 MPRIS item).
- [ ] In both VMs: fractional scaling at 125 % and 150 %, keyring storage of the login (GNOME Keyring, KWallet through Secret Service), and the reader window on Wayland (closes the Phase 0 reader and scaling items).
- [ ] In both VMs: measure memory as PSS from `/proc/<pid>/smaps_rollup` for idle, browsing, playing and the reader, and add the numbers to `docs/decisions/0002-stack.md`.

### Slim LGPL libmpv

The Windows package ships shinchiro's full libmpv: 115 MB of the 154 MB unpacked (62 MB zip), GPL, with video output, Vulkan, libplacebo, Lua, JavaScript and libbluray that an audiobook player never uses. Not scheduled yet.

- [ ] Own repository (for example `LaurinSorgend/libmpv-audio`) with a GitHub Actions workflow that builds libmpv for audio only. Runs on a schedule (weekly) and by hand; publishes a release only when the mpv or FFmpeg version changed, so releases stay few.
- [ ] Windows x64: cross-compile with mingw-w64, starting from shinchiro's `mpv-winbuild-cmake` with most packages turned off. Linux x86_64: build on the oldest Ubuntu the AppImage targets, for the AppImage only; distro and Flatpak builds keep using the system libmpv.
- [ ] mpv: `-Dgpl=false`, no Lua, JavaScript, libplacebo, Vulkan, OpenGL or other video outputs, no libbluray, libarchive, cdda, dvdnav. Keep the audio outputs (WASAPI on Windows; PulseAudio, PipeWire and ALSA on Linux) and `scaletempo2`.
- [ ] FFmpeg without `--enable-gpl` and `--enable-nonfree`, `--disable-everything`, then only what audiobooks and podcasts need:
  - decoders: AAC, MP3, Opus, Vorbis, FLAC, ALAC, AC-3/E-AC-3, WMA, PCM, plus MJPEG/PNG for embedded covers;
  - demuxers: MOV/MP4/M4B, Matroska, Ogg, MP3, FLAC, WAV, AAC/ADTS, ASF, HLS;
  - protocols: file, HTTP(S) with TLS from Schannel on Windows and GnuTLS or mbedTLS on Linux (no OpenSSL with `--enable-nonfree`);
  - filters: what mpv needs by default plus the ones the Audio effects section plans (`loudnorm`, `dynaudnorm`, `speechnorm`, `afftdn`, `anlmdn`, `arnndn`, `highpass`, `lowpass`, `equalizer`, `acompressor`, `silenceremove`).
- [ ] Release assets: `libmpv-2.dll` with `mpv.lib`/`libmpv.dll.a` and headers (Windows), `libmpv.so.2` (Linux), the license texts of mpv, FFmpeg and every library linked in, a manifest with the exact versions and configure flags, and SHA-256 sums.
- [ ] A smoke test in the same workflow before publishing: load the library, check `mpv_client_api_version` is 2.x, play an MP3, an M4B and an Opus file with `ao=null`, and fail on any of the filters above missing (`lavfi-list`).
- [ ] This repository: `scripts/fetch-libmpv.ps1` (and a Linux counterpart for the AppImage) downloads a pinned release and checks its SHA-256; bumping the pin is a normal commit, so CI and releases never pick up a new build by surprise. `packaging/licenses` takes the license texts from the release.
- [ ] Measure the DLL size and the playing memory against the shinchiro build before switching, and play every fixture through the app once.

**Done when:** the Windows zip uses the slim build, no GPL-only code is linked into libmpv, and playback, HLS, seeking and speed changes work as before.

### Audio effects

Applied while listening, on this machine only. Files on the server and downloads are never changed. Not scheduled yet; do after dogfooding.
- [ ] Built on libmpv's `af` property, which runs FFmpeg filters and can change during playback. `Player` gets `set_filters(&[Filter])`; `MpvPlayer` turns the list into an `af` string.
- [ ] Loudness: `loudnorm` (EBU R128, one pass) or `dynaudnorm` for books whose volume jumps between chapters or narrators. `speechnorm` as a third choice for quiet speech.
- [ ] Noise cleanup, with the algorithm selectable: `afftdn` (FFT denoiser, cheap), `anlmdn` (non-local means, more CPU), `arnndn` (RNNoise, needs a bundled `.rnnn` model).
- [ ] Hiss and rumble: `highpass` and `lowpass` with presets; a small voice equalizer (`equalizer` bands).
- [ ] Dynamics: `acompressor` for listening in noisy places.
- [ ] Optional silence trimming (`silenceremove`) with a threshold. The displayed position must stay in book time, so measure how mpv reports time with it on before shipping it.
- [ ] Settings: a global default plus an override per item, stored locally. A switch in the player to compare with and without effects.
- [ ] Check that the slim LGPL libmpv build includes these filters (its smoke test checks them). Measure CPU per filter on a low-end laptop and note the cost next to each choice.

### Transcripts and read-along

Depends on Phase 3. Not scheduled yet.
- [ ] Transcript: generated on this machine with whisper.cpp (`whisper-rs`), on request per item, with word timestamps. The model is a separate download the user chooses (size vs. accuracy). Runs in the background, can resume, and is cached per item and file hash.
- [ ] Transcript panel in the player: follows the position, highlights the current sentence, click to seek, search inside the book.
- [ ] Read-along when the item also has an ebook: highlight the passage being read and turn pages automatically.
- [ ] Alignment, in order of preference: EPUB 3 Media Overlays (SMIL) if the book has them; otherwise align the whisper transcript to the ebook text (fuzzy sequence alignment on normalized words), giving a map from audio time to CFI ranges. Cache the map per item.
- [ ] IPC additions for `abs-reader`: main -> reader `highlight {cfi}` and `follow {on}`; reader -> main `seek-request {cfi}` when the user clicks a sentence.
- [ ] Following stops when the user turns pages or scrolls by hand; a "back to audio" button resumes it.
- [ ] Measure transcription time and memory per hour of audio for each model size before deciding the default.

### Testing
- Unit tests in `core` and `abs-api` for anything with logic (token scheduling, time mapping, matcher, conflict rules).
- Integration tests against the oldest supported and the latest ABS in CI.
- When a new ABS release appears: read its release notes for "API" changes, bump the latest version in CI, fix breakages.

### Memory
- Request covers at about twice the tile size, in WebP.
- Decode only visible rows; cap the decoded cache by bytes.
- Page grid data from SQLite; don't hold the whole catalog in view models.
- Re-measure at the end of every phase against the Phase 0 budgets.

### Dependency risk
Single-maintainer or unstable dependencies each sit behind a small internal trait so they can be swapped:

| Dependency | Wrapped by | Fallback |
|---|---|---|
| souvlaki | `MediaControls` trait | `windows` crate SMTC + `mpris-server` |
| libmpv2 | `Player` trait | symphonia + rodio |
| tokio-tungstenite | `abs_api::Realtime` (own Engine.IO client, ADR 0003) | `tungstenite` directly or another WebSocket crate |
| foliate-js | IPC protocol | epub.js in the same helper |

### Open questions and when to answer them

| Question | Answer in |
|---|---|
| Do file download routes support `Range`? | 0.3 |
| Slint good enough, or switch to Qt? | 0.8 |
| What does the web reader store for PDF and comics? | 3.1 |
| How does session sync behave after a server restart? | 1.8: sync returns 404; starting a new session and syncing again works (tested on 2.37.2) |
| Does yaabsa already cover multi-account or OIDC? | before Phase 4 |
| Final license | 0.1 |
| Which whisper model size is the default for transcripts? | Transcripts and read-along |
