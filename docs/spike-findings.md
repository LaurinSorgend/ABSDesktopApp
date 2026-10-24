# Phase 0 spike findings

Measured against the Docker test servers (ABS 2.26.3 and 2.37.2) with `cargo run -p abs-api --example smoke`.

## API (0.3)

- `POST /login` with `x-return-tokens: true` returns `user.accessToken` and `user.refreshToken` in the body on both versions. The legacy `user.token` is still present.
- `POST /auth/refresh` with `x-refresh-token` works on both versions. 2.37.2 returns a new refresh token; 2.26.3 returns the same one. The client must store whatever comes back and must not assume rotation.
- `GET /api/items/:id/file/:ino/download` honours `Range`: `206 Partial Content`, `Content-Range: bytes 100-1123/<size>`, `Accept-Ranges: bytes` on both versions. Phase 2 downloads can resume with `Range`.
- `GET /api/items/:id/cover?width=400&format=webp` returns `image/webp`. Items without a cover return 404.
- `GET /api/libraries/:id/items?minified=1&limit=&page=` returns `results` and `total`.
