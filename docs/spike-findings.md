# Phase 0 spike findings

Measured against the Docker test servers (ABS 2.26.3 and 2.37.2) with `cargo run -p abs-api --example smoke`.

## API (0.3)

- `POST /login` with `x-return-tokens: true` returns `user.accessToken` and `user.refreshToken` in the body on both versions. The legacy `user.token` is still present.
- `POST /auth/refresh` with `x-refresh-token` works on both versions. 2.37.2 returns a new refresh token; 2.26.3 returns the same one. The client must store whatever comes back and must not assume rotation.
- `GET /api/items/:id/file/:ino/download` honours `Range`: `206 Partial Content`, `Content-Range: bytes 100-1123/<size>`, `Accept-Ranges: bytes` on both versions. Phase 2 downloads can resume with `Range`.
- `GET /api/items/:id/cover?width=400&format=webp` returns `image/webp`. Items without a cover return 404.
- `GET /api/libraries/:id/items?minified=1&limit=&page=` returns `results` and `total`.

## Cover grid (0.4)

`cargo run -p abs-desktop --example cover_grid --release`, Slint 1.18, Windows 11, 500-item Grid library, 6 columns, covers requested at 320 px WebP and decoded to RGBA on the tokio blocking pool. Decoded covers sit in a 64 MB byte-capped LRU. `SPIKE_AUTOSCROLL=1` scrolls top to bottom and back for 30 s. Private working set, sampled every 2 s:

| Renderer | Idle, 36 covers on screen | Scrolling, cache full |
|---|---|---|
| winit-software | 26.5 MB | 83.2 MB |
| winit-femtovg (default) | 89.4 MB | 139.9 MB peak |

- The GPU renderer costs about 60 MB on top of the software renderer, mostly driver state and texture copies of the cached images.
- Skia isn't compiled in by default (`renderer-skia` feature); `SLINT_BACKEND=winit-skia` falls back to FemtoVG.
- One decoded 320 px cover is 400 KB, so 64 MB holds about 160 covers. Scrolling a 500-item library twice decoded 765 covers. A smaller decode size (exact physical tile size) or a 32 MB cap are the obvious levers.
- Open: scrolling smoothness, HiDPI on Windows and fractional scaling on Wayland still need a manual check.

## libmpv playback (0.5)

`cargo run -p abs-player --example mpv_spike --release` against ABS 2.37.2 on localhost, `ao=null`, libmpv from shinchiro's build 20261021 (`scripts/fetch-libmpv.ps1`).

- Direct play works with `http-header-fields=Authorization: Bearer <token>` and the session's `contentUrl` prefixed with the server URL.
- Multi-file book: tracks loaded as a playlist (`loadfile replace`, then `append`). Mapping global time to `(track, offset)` with `startOffset` and switching via `playlist-pos` then `seek absolute` landed exactly on the target for 5 %, 45 % and 90 %. Switching tracks emits `END_FILE` with reason `STOP` (2), which isn't an error.
- 40 h M4B: open to first audio 462 ms; seeks to 10 %, 50 % and 95 % each took 23-25 ms until `PLAYBACK_RESTART`. This is localhost; a remote server adds its round trip time per seek.
- Speed 0.75x to 3.0x: audio advances at the expected rate and mpv logs `[autoaspeed] adding scaletempo2`, so pitch is preserved without extra configuration.
- The shinchiro DLL is 116 MB with everything enabled (Vulkan, libplacebo, Lua, JavaScript, libbluray, ...) and is GPL. It also has `win32-smtc`, mpv's own Windows media controls, which the app must turn off (`media-controls=no`) or use instead of souvlaki (see 0.6).

Private working set (`mpv_idle` example, one URL, `ao=null`):

| Case | Private WS |
|---|---|
| libmpv loaded, idle handle | 6.8 MB |
| MP3 track, default cache (150 MiB forward) | 67.4 MB |
| MP3 track, `demuxer-max-bytes=8MiB`, `demuxer-max-back-bytes=2MiB` | 24.5 MB |
| 40 h M4B, default cache | 335.0 MB |
| 40 h M4B, 8 MiB cap | 133.9 MB |
| 40 h M4B, `cache=no` | 122.7 MB |

- The default demuxer cache downloads up to 150 MiB ahead; capping it is required.
- The 40 h M4B costs about 110 MB more than an MP3 track no matter the cache settings. That is FFmpeg's MP4 demuxer keeping its sample tables and index for all ~3.1 million AAC frames in memory. A 20 h M4B at 44.1 kHz has the same frame count, so this hits common single-file books, not only the fixture.
- Disabling libmpv's built-in Lua scripts (`load-scripts=no` and friends) made no measurable difference.

Options for the M4B index cost, to decide in 0.8: accept it in the playing budget; ask ABS to transcode to HLS for long single-file books (more server CPU); or measure the symphonia fallback on the same file.
