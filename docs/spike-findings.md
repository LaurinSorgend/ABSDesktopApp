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
