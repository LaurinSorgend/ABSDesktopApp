# Architecture, data and distribution for a native Audiobookshelf desktop client (Windows + Linux)

Research date: 2026-10-24. Source checking covered about 20 tool calls. Items under "Inferences" that carry no citation come from general engineering knowledge and were not re-verified in this session. The report writer should treat them as recommendations, not sourced facts.

## 1. Credential and token storage (per account, cross-platform)

### Takeaway
ABS (since v2.26.0) uses JWT access tokens plus rotating refresh tokens. A native client should send `x-return-tokens: true` on login, store only the refresh token per account in the OS secret store (Windows Credential Manager / Linux Secret Service), and keep the access token in memory. In Rust, use keyring 4.x (now split into keyring-core plus per-store crates). In Qt, use QtKeychain. Plan an explicit, opt-in fallback for Linux systems with no Secret Service.

### Cited findings
- ABS auth since v2.26.0: short-lived JWT access token (default 12 h, `ACCESS_TOKEN_EXPIRY`) and rotating refresh token (default 7 days, `REFRESH_TOKEN_EXPIRY`). `POST /login` returns `user.accessToken`. Non-cookie clients send the header `x-return-tokens: true` to get the refresh token in the body. On 401, call `POST /auth/refresh` with the `x-refresh-token` header, then retry. `POST /logout` takes `x-refresh-token`. API keys (`/api/api-keys`) are meant for scripts, not third-party apps. Auth endpoints are rate-limited per IP (default 40 requests per 600000 ms). The old token system is to be removed no earlier than 2025-09-30. — [ABS discussion #4460](https://github.com/advplyr/audiobookshelf/discussions/4460)
- A Cloudron changelog for ABS 2.31.0 says default token expirations went up to 1 h (access) and 30 days (refresh). This conflicts with the 12 h / 7 d defaults in #4460, so both changed over time. Handle the expiry the server gives you and don't hard-code it. — [Cloudron forum](https://forum.cloudron.io/topic/9434/audiobookshelf-package-updates?page=5)
- Recent ABS auth changes: v2.35.0 added an access-token refresh grace period. v2.35.1 fixed duplicate refresh tokens across sessions that caused unexpected logouts. v2.36.0 added an auth sessions table with per-session logout and `/logout?allDevices=1`; a password change now invalidates all sessions; the refresh-token grace period became 10 min (`REFRESH_TOKEN_GRACE_PERIOD`); and refresh tokens are no longer accepted as bearer tokens on API/websocket. v2.37.0 lets API keys authenticate websockets. The latest release is v2.37.2 (23 Oct). — [ABS releases](https://github.com/advplyr/audiobookshelf/releases)
- CVE-2026-73085: refresh-type JWTs were accepted as bearer tokens on API/WebSocket endpoints, fixed in 2.36.0. Mend lists the affected range as 2.35.0–2.36.0, while exploit-intel says "prior to 2.36.0". — [Mend](https://www.mend.io/vulnerability-database/CVE-2026-73085/)
- Rust `keyring`: latest is 4.2.0 (2026-08-29). The default `v1` feature keeps the old API. The library/API moved into `keyring-core`, and each credential store is now its own crate: `windows-native-keyring-store`, `dbus-secret-service-keyring-store`, `zbus-secret-service-keyring-store`, `linux-keyutils-keyring-store`. Apps should depend on `keyring-core` plus the stores they need. Avoid the `cli` feature, which pulls in many stores. Maintainer is Dan Brotsky. License is MIT/Apache-2.0. — [docs.rs keyring](https://docs.rs/crate/keyring/latest); [keyring-rs repo](https://github.com/open-source-cooperative/keyring-rs)
- The docs.rs page does not say whether the Secret Service and keyutils stores fall back to each other. — [docs.rs keyring](https://docs.rs/crate/keyring/latest)
- Windows Credential Manager limits: `CredentialBlob` max is `CRED_MAX_CREDENTIAL_BLOB_SIZE` = 5*512 = 2560 bytes. Generic `TargetName` can be up to 32767 chars. Use `CRED_PERSIST_LOCAL_MACHINE` (or `ENTERPRISE`, which roams with roaming profiles). Microsoft suggests prefixing generic target names with the vendor/app name. — [Microsoft Learn CREDENTIALW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw)
- QtKeychain: on Windows it uses the Credential Store by default, or `CryptProtectData` (DPAPI) plus QSettings with `-DUSE_CREDENTIAL_STORE=OFF`. On Linux it uses GNOME Keyring if running, else KWallet over D-Bus, and libsecret is also supported. Qt 5 and Qt 6 are supported from 0.12. It refuses to store plaintext unless `setInsecureFallback(true)` is set. — [QtKeychain repo](https://github.com/frankosterfeld/qtkeychain)
- oo7 (Rust, MIT) implements the freedesktop Secret Service: a client library, an `oo7-daemon` server, and an `org.freedesktop.impl.portal.Secret` portal for sandboxed apps, with KWallet file migration. — [oo7 repo](https://github.com/bilelmoussaoui/oo7)

### Inferences
- Key each secret by a stable local account UUID, e.g. service `ABSDesktop`, user `account:<uuid>`, not by URL+username. Then the "same server, different users" and "server URL changed" cases don't break lookups. A JWT refresh token is well under 2560 bytes, but don't put multiple accounts in one blob.
- Refresh tokens rotate on each use (#4460). Write the new refresh token to the keyring before the old one is dropped, and serialize refreshes per account (one in-flight refresh, others await it). Otherwise two parallel 401s can burn the token. The 10-minute grace period from v2.36.0 makes this more forgiving but doesn't remove the need.
- Linux with no Secret Service (headless, minimal WMs): detect the failure, then offer (a) the kernel keyutils store, which keyring has as a crate but which doesn't survive reboot (general knowledge, unverified here), or (b) an explicit opt-in encrypted or plaintext file in `$XDG_DATA_HOME` with 0600 permissions, or (c) "don't remember me". Inside Flatpak, the Secret portal (oo7-portal / xdg-desktop-portal) is the route. The oo7 client crate handles portal vs D-Bus in Rust (general knowledge of oo7, not confirmed on the fetched page).
- If OIDC login is supported, ABS has an OIDC mobile flow (v2.37.1 notes mention it). Implementing it on desktop needs a loopback or custom URI scheme redirect. Defer it to a later version.

### Gaps
- Could not read the exact current default token lifetimes from ABS source. Sources conflict (12 h/7 d vs 1 h/30 d).
- Did not confirm keyring 4.x feature flag names or default store selection per platform.

## 2. Local storage: SQLite schema, cover cache, HTTP caching

### Takeaway
Use one SQLite database per app profile, with every cached row keyed by `(account_id, remote_id)`. Keep covers on disk in a size-capped LRU directory, never in the DB. Fetch thumbnails at display size, since the ABS cover endpoints accept width/height/format.

### Cited findings
- ABS cover and author image cache endpoints take width/height (they must be positive since v2.37.0), and cover format is limited to `webp`, `jpeg`, `png` (v2.36.1). — [ABS releases](https://github.com/advplyr/audiobookshelf/releases)
- ABS v2.36.0 added `GET /api/me/progress`, `/api/me/bookmarks`, `/api/me/sessions`, and expanded library item JSON with all minified fields. These are useful for an initial cache fill. — [ABS releases](https://github.com/advplyr/audiobookshelf/releases)
- The `POST /login` response includes `user.mediaProgress[]` with `currentTime`, `duration`, `progress`, `isFinished`, `lastUpdate`. The old API docs site says it is "out-of-date and no longer maintained". — [api.audiobookshelf.org](https://api.audiobookshelf.org/)

### Inferences (unverified engineering recommendations)
- Rust options: `rusqlite` (sync, bundled SQLite, simplest, smallest), `sqlx` (async, compile-time-checked queries, needs offline query data in CI), SeaORM (heavier ORM on top of sqlx). For a desktop client with a single-writer DB, rusqlite plus a dedicated DB thread (or `tokio::task::spawn_blocking`) plus `refinery`/`rusqlite_migration` for migrations is the leanest. Qt: Qt SQL with the QSQLITE driver.
- Turn on WAL mode, `foreign_keys=ON`, and FTS5 for local search across merged libraries.
- Suggested tables: `account(id uuid, server_id, display_name, username, user_id_remote, last_sync_at)`, `server(id, base_url, server_version, alt_urls)`, `library(account_id, remote_id, name, media_type)`, `item(account_id, remote_id, library_remote_id, title, title_sort, author_display, asin, isbn, duration_s, updated_at_remote, json_blob)`, `progress(account_id, item_remote_id, episode_id, current_time, is_finished, updated_at, dirty)`, `pending_session(local_id, account_id, payload_json, created_at, attempts)`, `download(id, account_id, item_remote_id, state, bytes_done, bytes_total, path)`, `work(id)` plus `work_member(work_id, account_id, item_remote_id, match_method, confidence)` for the merge layer (section 3). Keep the raw server JSON in a column so new fields don't require migrations.
- A delta refresh can use library item `updatedAt` plus socket events (`item_updated`, `item_removed` with `libraryId` since v2.33.2, `user_item_progress_updated`) to avoid full re-fetches.
- Cover cache: path `cache/covers/<account_id>/<item_id>_<w>.webp`, an LRU index in SQLite (size, last_access), a configurable cap (e.g. 300 MB), and eviction on startup or when the cap is exceeded. Use the platform cache directory (`%LOCALAPPDATA%\<app>\cache`, `$XDG_CACHE_HOME/<app>`), e.g. via the `directories` crate. For HTTP caching, store ETag/Last-Modified and use conditional requests. `http-cache-reqwest` exists, but a hand-rolled disk cache for covers is simpler and gives exact size control.

### Gaps
- Did not verify whether ABS cover endpoints return ETag/Cache-Control headers.

## 3. Library merge model across accounts

### Takeaway
None of the multi-server music clients checked (Supersonic, Feishin, Symfonium) truly merges libraries. They switch between servers or filter globally, and Symfonium says cross-provider dedup is hard. The safe model is a two-layer one. The source of truth is per account. On top sits an optional, user-correctable "work" layer that groups items by strong IDs (ASIN/ISBN) first and fuzzy title+author+duration second. Progress always stays tied to a specific (account, item), because ABS stores progress per user per server.

### Cited findings
- Supersonic (Go + Fyne) lets you "add and switch between multiple servers". Merging isn't documented. — [Supersonic repo](https://github.com/dweymouth/supersonic)
- Feishin (Electron) keeps a list of servers via "Manage servers" and prompts you to select one at startup, so one server is active at a time. — [Feishin repo](https://github.com/jeffvli/feishin)
- Symfonium: adding the same Navidrome server twice (Tailscale IP and LAN IP) duplicated every track. Support's answer was to add a secondary connection to one provider, not a second provider. — [Symfonium support](https://support.symfonium.app/t/issue-with-duplicate-tracks-and-missing-network-restriction-settings-in-version-14-0-0/13334)
- Symfonium on cross-provider duplicates: duplicate handling "is hard without proper hashes and even more when dealing with multiple providers". — [Symfonium support](https://support.symfonium.app/t/option-to-blend-off-duplicates/3362)
- Symfonium supports global filtering of displayed data by provider, library, and offline media. — [Symfonium docs](https://docs.symfonium.app/wiki/providers/)
- ABS's local-session sync remaps `libraryItemId` and fills `userId`/`libraryId` server-side, so progress is strictly per user on that server. — [PlaybackSessionManager.js](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)

### Inferences
- Model a "server" separately from an "account" (server plus user). Give a server multiple URLs (LAN, VPN, public), following Symfonium's secondary-connection lesson, so one server reached two ways isn't treated as two sources.
- Offer three views: per account (default), merged (works grouped), and filtered (pick accounts and libraries).
- Dedup cascade, most to least reliable: (1) same server and same item id under two users is an exact match, because the item is identical. (2) ASIN equal (normalized uppercase). (3) ISBN-13 equal (convert ISBN-10 to 13). (4) Normalized title (lowercase, strip punctuation, articles, subtitle after ":", series markers) plus normalized primary author, with total duration within about 2% or 3 min. Store `match_method` and `confidence`, let the user split or merge manually, and persist overrides so a re-sync doesn't undo them. Never auto-merge on title alone. Different narrators or abridged editions differ in duration.
- Picking the playback source for a merged work: if one member has the "active" progress (most recent `updatedAt`), play from it. Otherwise prefer a local download, then the fastest-reachable server, then a user-set preferred account. Progress is written only to the account the session plays from. Optionally offer "mirror progress to other members". This is a client-side copy via `PATCH /api/me/progress/:id` and should be off by default, because a chapter-level timestamp may not map across different editions. Only mirror when durations match closely.
- Within one ABS server, ABS stores a book once per library. Two users on the same server see the same libraryItemId, which makes case (1) cheap.

### Gaps
- Did not examine Jellyfin clients, Finamp, Plexamp, or Infuse multi-server behavior in source. Plex's own server-side aggregation was not researched.
- No sourced dedup heuristic from an existing audiobook client was found.

## 4. Download manager

### Takeaway
Download per-file (each audio track of a library item) with HTTP Range resume, a small global concurrency limit, temp-file plus atomic rename, and size/duration verification. Record state in SQLite so downloads survive restarts.

### Cited findings
- I found no sourced material in this session on ABS download endpoints' Range support or on Rust download crates. See Gaps.

### Inferences (unverified)
- ABS serves item files via `/api/items/:id/file/:ino` (and `/download` for a zip). The per-file route is better for resume and partial availability. Verify Range support against a test server.
- Rust: `reqwest` (rustls, `stream` feature) on `tokio`. Write chunks to `<name>.part`, persist `bytes_done` periodically, resume with `Range: bytes=N-` and check for `206` plus a matching `Content-Range`/ETag (restart on `200`). Use a `tokio::sync::Semaphore` for concurrency (2–3 files globally, 1 per server), and retry with exponential backoff. Verify the final size against the item's audioFile `metadata.size`. ABS doesn't expose a hash, so size plus optionally an ffprobe-free duration check is the realistic check. `fsync`, then rename.
- Storage layout: `<downloads>/<account_id>/<library_item_id>/<ino>.<ext>` plus a `manifest.json` (item JSON, chapters, cover). The item stays playable offline, and the DB can be rebuilt from manifests. Let the user move the downloads root.
- "Background downloads": desktop apps can keep running in the tray. A separate service isn't needed. On Windows, use `SetThreadExecutionState` to prevent sleep during active downloads if wanted.
- Qt alternative: QNetworkAccessManager with a `Range` header and QSaveFile.

### Gaps
- Did not confirm ABS Range/ETag behavior on file routes or the exact current download route names in the OpenAPI spec.

## 5. Offline progress sync and conflict resolution

### Takeaway
ABS already implements server-side last-write-wins for offline sessions. `POST /api/session/local` and `/api/session/local-all` accept locally recorded sessions and update media progress only if the session's `updatedAt` is newer than the server's progress `updatedAt`. The client should queue full local sessions in SQLite and batch-flush them on reconnect.

### Cited findings
- `syncLocalSessionsRequest` reads `req.body.sessions[]` and returns `{ results: [...] }`, with each entry `{id, success:true, progressSynced}` or `{id, success:false, error}`. One failure doesn't abort the batch. The single-session route returns a bare 200 or a 500 with the error, so `progressSynced` is not returned there. — [PlaybackSessionManager.js](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)
- Fields used: `id`, `libraryItemId`, `episodeId`, `currentTime`, `timeListening`, `updatedAt`, `displayTitle`. IDs prefixed `play_local_` are replaced with server UUIDs. `bookId`/`libraryId`/`userId` are filled in by the server. — [same](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)
- Conflict rule: `if (userProgressForItem.updatedAt.valueOf() > session.updatedAt)` skips the update. Otherwise the server writes progress and emits `user_item_progress_updated` to the user's other clients. — [same](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)
- Errors returned: `'Media item not found'`, `'Library not found'`. — [same](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)
- The old API docs list "Sync an Open Session" (`/api/session/:id/sync`) and "Batch Create/Update Media Progress" (`/api/me/progress/batch/update`), but the docs are unmaintained. — [api.audiobookshelf.org](https://api.audiobookshelf.org/)

### Inferences
- Online playback: open a session with `POST /api/items/:id/play`, then `POST /api/session/:id/sync` every 10–15 s and on pause/seek/close, and `/close` at the end. Offline: create a `play_local_<uuid>` session record locally, update it at the same cadence in SQLite, and flush via `/api/session/local-all` when connectivity returns. Mark rows synced only when `success:true`. Keep rows with `success:true, progressSynced:false` and surface "server had newer progress", then pull that progress.
- `updatedAt` is compared against the client's clock, so clock skew can make LWW wrong. Mitigate by recording each server's clock offset from the HTTP `Date` header at login/refresh and adjusting outgoing `updatedAt` by that offset.
- After the flush, re-pull `/api/me/progress` (v2.36.0+) and reconcile local state. Subscribe to the socket `user_item_progress_updated` to catch progress from phones.
- Edge case: if the user listened offline on the desktop and also on their phone, LWW picks the latest write, which may not be the furthest position. Consider a UI prompt when the two positions differ by more than N minutes, offering "Continue from 3:12:04 (desktop) or 4:01:55 (phone)". Some podcast clients do this.

### Gaps
- Did not verify how `PlaybackSession` sets `updatedAt` for newly created local sessions, or whether `local-all` requires a minimum server version.

## 6. Packaging, code signing and auto-update

### Takeaway
On Windows, ship a per-user installer with built-in delta updates (Velopack) or MSI/NSIS via cargo-packager, plus winget/Scoop manifests and a portable zip. Sign through SignPath Foundation (free for OSS) or Azure Artifact Signing (about $9.99/month, but individuals only in US/Canada). On Linux, make Flathub the primary channel (it handles updates; build from source with no network), with AppImage, .deb, and AUR as extras.

### Cited findings
- Velopack: MIT, written in Rust, installer and auto-update framework for Windows/macOS/Linux, delta packages, updates hosted on any static file server including GitHub Releases. Has a Rust crate (`velopack` on crates.io, docs.rs) and also supports C#/C++/JS. — [Velopack repo](https://github.com/velopack/velopack); [Velopack docs](https://docs.velopack.io/)
- cargo-packager (CrabNebula): outputs NSIS .exe and WiX MSI on Windows; .deb, AppImage, and Pacman PKGBUILD on Linux; DMG/.app on macOS. Companion updater crate `cargo-packager-updater`. Examples include Slint, egui, dioxus, and wry. Iced isn't listed, but the tool packages any built binary. — [cargo-packager repo](https://github.com/crabnebula-dev/cargo-packager)
- cargo-dist (axodotdev): active (about 3.5k commits, self-hosting). `dist init` generates a GitHub Actions `release.yml` with plan/build/host/publish/announce stages. The fetched page didn't list its installer types or updater. — [cargo-dist repo](https://github.com/axodotdev/cargo-dist)
- Azure Artifact Signing (formerly Trusted Signing): about $9.99/month (Basic, 5,000 signatures/month; Premium 100,000). Individual developers are eligible only in the USA/Canada, organizations also in the EU/UK. Identity validation takes days. — [Microsoft Learn code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options); [Azure pricing](https://azure.microsoft.com/de-de/pricing/details/artifact-signing/)
- SignPath Foundation: free code signing for OSS. The binary must be built from the public repository, SignPath vouches under its own certificate (not your identity), and the key sits on SignPath's HSM. — [signpath.org](https://signpath.org/)
- Flathub requirements: everything built from source, all dependencies listed as manifest sources (no network at build time, so Rust needs vendored crates via flatpak-cargo-generator), minimal static permissions, mandatory portals where they exist, a validated AppStream MetaInfo file, a correct license, app-ID domain verification via `/.well-known/org.flathub.VerifiedApps.txt`, and complete English localisation. AI-generated material must be disclosed, and manifests may not contain AI-generated or AI-assisted content. — [Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)

### Inferences
- The author is in the EU (laurin@sorgend.eu). As an individual, Azure Artifact Signing is likely not available, so SignPath Foundation is the realistic free option. It requires a public OSS repo and build provenance from CI. Unsigned builds trigger SmartScreen warnings. winget accepts unsigned installers, but users still see SmartScreen.
- Don't run two updaters. Velopack self-updates for direct downloads. winget, Scoop, Flatpak, AUR, and distro packages update through their own channel. Detect the install channel at runtime (e.g. env var or marker file set at build) and turn off in-app updates for package-manager installs.
- MSIX is possible but adds Store-style identity and sandbox quirks. Low priority for a v1.
- AppImage: Velopack's Linux output is (by general knowledge) an AppImage with self-update. Confirm in the Velopack packaging docs. Flathub needs the Flatpak manifest to vendor all crates and, for Rust GUI toolkits, include the needed GL/Wayland libs from the GNOME/KDE/Freedesktop runtime.
- The Flathub rule against AI-assisted manifest content matters if AI tools are used in development. Write the manifest by hand.

### Gaps
- Did not get current Velopack/cargo-dist/cargo-packager version numbers, or whether cargo-dist supports MSI/GUI apps well.
- SignPath Foundation's detailed eligibility (license types, project maturity) wasn't retrieved because the about.signpath.io page failed to load.

## 7. CI (GitHub Actions)

### Takeaway
Use native runners (`windows-latest`, `ubuntu-latest`) instead of cross-compiling GUI apps. Use cargo-dist or a hand-written workflow for release artifacts, plus a job that runs integration tests against an ABS Docker container on the Linux runner.

### Cited findings
- cargo-dist generates a release workflow (plan, build, host, publish, announce) on `dist init`. — [cargo-dist repo](https://github.com/axodotdev/cargo-dist)
- SignPath requires the signed binary to be built from the OSS repository, which in practice means a CI-built artifact. — [signpath.org](https://signpath.org/)

### Inferences (unverified)
- Matrix: `{windows-latest: x86_64-pc-windows-msvc, ubuntu-22.04: x86_64-unknown-linux-gnu}`. Build on the oldest supported Ubuntu for glibc compatibility. Add aarch64 Linux later via `ubuntu-24.04-arm` runners.
- Steps: `Swatinem/rust-cache`, `cargo fmt --check`, `clippy -D warnings`, `cargo nextest`, then a release job on tag that packages (Velopack `vpk pack` or cargo-packager), signs (SignPath GitHub action), and uploads to GitHub Releases. Run the Flathub build separately in the flathub repo's own CI.
- Integration tests: service container `ghcr.io/advplyr/audiobookshelf:<pinned version>`. Seed it via the API (`POST /init` creates the root user, then create a library pointing at a mounted fixture folder of small CC0 audio files and trigger a scan). Test login/refresh, library fetch, progress sync, and the local-session LWW rule against at least the oldest supported version (2.26.0) and latest. v2.37.2 rejects creating a second root user on init, so re-create the container per run.

### Gaps
- Not verified: the exact ABS Docker image name/tag and the `/init` endpoint payload.

## 8. Maintainability: structure, API client, localization

### Takeaway
Put all logic in a UI-free core library (API client, DB, sync, downloads, merge), with the GUI as a thin layer over an async command/event API. ABS ships an OpenAPI 3.0 spec (`docs/openapi.json`), but it was built up endpoint by endpoint and is marked version 0.1.0. Use it as a reference or for model generation, and hand-write the small set of endpoints the client needs.

### Cited findings
- The ABS repo `docs/` contains `openapi.json` (bundled), `root.yaml`, `schemas.yaml`, `controllers/`, and `objects/`. The YAML files mirror the server source hierarchy, and the README covers linting/validation but makes no completeness claim. — [ABS docs folder](https://github.com/advplyr/audiobookshelf/tree/master/docs)
- The spec root is openapi 3.0.0, "Audiobookshelf API", version 0.1.0, pointing at a localhost dev server. Commits added library, author, series, and library-item endpoints incrementally (starting with "Initial library endpoints (#3012)"). — [git.laurivan.com mirror](https://git.laurivan.com/Mirrors/audiobookshelf/blame/tag/v2.18.0/docs/root.yaml)
- The legacy API docs site says it is out of date and unmaintained. — [api.audiobookshelf.org](https://api.audiobookshelf.org/)

### Inferences
- Rust workspace sketch: `crates/abs-api` (typed reqwest client, serde models, auth/refresh middleware, no DB), `crates/core` (accounts, SQLite repo, sync engine, download manager, merge layer, exposes `Command`/`Event` channels), `crates/player` (audio backend, e.g. GStreamer or symphonia+cpal, or libmpv), and `app/` (Slint/iced/GTK UI). The core must build and test without a display.
- API client: hand-write it with serde models tolerant of unknown fields (`#[serde(default)]`, no `deny_unknown_fields`), since ABS adds fields often (e.g. v2.36.0's expanded item JSON). If generating from openapi.json, `progenitor` (Oxide) or `openapi-generator`'s Rust target can produce models. Expect missing endpoints (sessions, auth refresh) to need hand-writing. Record contract tests against the Docker instance to catch drift.
- Detect the server version at login (`serverSettings.version`) and gate features, e.g. `/api/me/progress` needs ≥2.36.0 and JWT needs ≥2.26.0.
- Localization: Fluent (`fluent-bundle` plus `fluent-langneg`, or `i18n-embed`) fits Rust well. Slint has built-in gettext-style `@tr()`. GTK and Qt use gettext and Qt Linguist (.ts) respectively. Flathub requires complete English strings.

### Gaps
- Did not measure what fraction of server routes openapi.json covers. No GitHub issue on spec completeness was found.

## 9. Memory efficiency

### Takeaway
Keep the working set small: request covers at thumbnail size from the server, decode lazily for visible rows only, use virtualized lists, and keep large data in SQLite rather than in RAM.

### Cited findings
- ABS cover endpoints accept width/height and webp/jpeg/png format, so clients can request display-sized thumbnails. — [ABS releases](https://github.com/advplyr/audiobookshelf/releases)

### Inferences (unverified)
- Request about 2x the display size (e.g. 240 px for a 120 px grid tile) in webp and cache that size on disk. Decode on a worker thread and hold an in-memory LRU of decoded textures capped by bytes (e.g. 64 MB), not by count.
- Use virtualized lists: Slint `ListView` instantiates only visible delegates, iced needs a virtual-list widget or manual windowing, GTK4 `GtkListView`/`GtkGridView` with `GListModel` are recycled views, Qt has `QAbstractItemModel` plus `QListView`/QML `ListView`. Page library queries from SQLite (`LIMIT/OFFSET` or keyset) instead of loading every item into the UI model.
- Keep large item JSON in SQLite and parse it on demand. Store chapters in a separate table. Stream downloads to disk, never buffer whole files. Prefer rustls over OpenSSL for smaller binaries and simpler Linux packaging.

### Gaps
- No benchmarks gathered on memory use of Slint vs iced vs GTK4 vs Qt for large grids.
