# Status

Last updated 2026-10-24.

## Phase 0: done on Windows, Linux open

Windows parts of Phase 0 are finished and checked by hand. Results are in `docs/spike-findings.md`, the stack decision draft in `docs/decisions/0002-stack.md` (status: proposed until Linux numbers exist).

Open before Phase 0 exit criteria are met:

- Linux (GNOME and KDE on Wayland): system libmpv, MPRIS via souvlaki, `abs-reader` through `build_gtk`, fractional scaling in Slint, memory as PSS from `/proc/<pid>/smaps_rollup`. Needs a Linux machine.
- LGPL libmpv build for Windows releases. The dev build (`scripts/fetch-libmpv.ps1`, shinchiro 20261021) is GPL and 116 MB.
- CI has never run: there is no GitHub remote yet.
- Measure the symphonia fallback on the 40 h M4B before Phase 1.7 (libmpv/FFmpeg costs about 110 MB extra for long single-file M4B because of the MP4 sample index).

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

Next: 1.3 storage in `core`.
