# Audiobookshelf server API for a native desktop client, and the existing client ecosystem

Research date: 2026-10-24. Server source examined: `advplyr/audiobookshelf` master branch, `package.json` version **2.37.2** (released 2026-10-23). Most findings below come from reading the server source directly, which is more reliable than the official API docs (those are explicitly unmaintained, see the versioning section).

Source shorthand (all on GitHub, master branch):
- ApiRouter = https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js
- Auth = https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js
- TokenManager = https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js
- OidcAuthStrategy = https://github.com/advplyr/audiobookshelf/blob/master/server/auth/OidcAuthStrategy.js
- PlaybackSessionManager = https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js
- SocketAuthority = https://github.com/advplyr/audiobookshelf/blob/master/server/SocketAuthority.js
- Server = https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js
- LibraryItemController = https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js
- LibraryController = https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryController.js
- Discussion 4460 (new JWT auth) = https://github.com/advplyr/audiobookshelf/discussions/4460

## Authentication (password login, API keys, access/refresh tokens, OIDC, expiry, multi-server/multi-user)

### Takeaway
Since v2.26.0 (2025-07-12) ABS uses short-lived JWT access tokens plus rotating refresh tokens; a native client logs in with `POST /login` and header `x-return-tokens: true`, stores the refresh token itself, and refreshes via `POST /auth/refresh` with `x-refresh-token`. Defaults are now 1 h access / 30 days refresh. OIDC for native apps is a PKCE (S256) flow where the client must reuse the same cookie jar for `/auth/openid` and `/auth/openid/callback`, and the redirect URI must be whitelisted by the server admin. Auth is entirely per-token, so multiple servers and multiple users on one server are just multiple independent token sets on the client side.

### Cited Findings

Login and token scheme
- v2.26.0 (2025-07-12) added "JWT authentication with refresh tokens to replace old authentication system", configurable API keys, a DB-backed Session model, and a rate limiter on auth endpoints; it also removed the `TOKEN_SECRET` env var. Users had to re-login; the old system was to remain "no earlier than September 30, 2025" — [v2.26.0 release](https://github.com/advplyr/audiobookshelf/releases/tag/v2.26.0)
- Every release up to v2.37.2 still carries the banner "Important: New authentication system was added in v2.26.0. See #4460" — [v2.37.2 release](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.2)
- `POST /login` body `{ username, password }`. Header `x-return-tokens: true` makes the server return `user.refreshToken` in the JSON body; otherwise the refresh token is only set as an HTTP-only `refresh_token` cookie. Response contains `user.accessToken` (new JWT) and still `user.token` (old-style token) "to keep backwards compatibility" — [Discussion 4460, endpoints comment](https://github.com/advplyr/audiobookshelf/discussions/4460#discussioncomment-13735737); confirmed in code `router.post('/login', ...)` reading `req.headers['x-return-tokens'] === 'true'` — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js)
- Login response payload also includes `userDefaultLibraryId`, `serverSettings`, `ereaderDevices`, `Source` — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js) (`getUserLoginResponsePayload`)
- `POST /auth/refresh`: reads refresh token from cookie, or from `x-refresh-token` header if present (header wins). Returns the same payload as login; if `x-refresh-token` was used the new refresh token is in the body. Refresh tokens rotate on each use — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js); [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460)
- `POST /logout` with `x-refresh-token` invalidates the session's refresh token; response may contain `redirect_url` for OIDC end-session logout — [Discussion 4460 comment](https://github.com/advplyr/audiobookshelf/discussions/4460#discussioncomment-13735737); [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js)
- Note these auth routes are at the server root (`/login`, `/auth/refresh`, `/logout`, `/auth/openid...`), not under `/api` — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js) (routes registered on the root router)
- Expected client behaviour: on 401, call `/auth/refresh`, then retry the original request — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460)
- Current defaults in code: `REFRESH_TOKEN_EXPIRY` 30 days, `ACCESS_TOKEN_EXPIRY` 1 hour, `REFRESH_TOKEN_GRACE_PERIOD` 10 minutes (all env-overridable, seconds) — [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js)
- History of the defaults: initial 2.26.0 defaults were access 12 h / refresh 7 days — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460); v2.31.0 (2025-12-01) "Increase default access & refresh token expirations. Refresh now 30 days & Access now 1 hour" — [v2.31.0 release](https://github.com/advplyr/audiobookshelf/releases/tag/v2.31.0)
- v2.35.0 (2026-05-17) added an access-token refresh grace period ("fixes frequently needing to re-login" #4630); v2.35.1 fixed "Duplicate refresh tokens across sessions can cause unexpected logout" (#5253); v2.36.0 (2026-07-27) extended the grace period to 10 min and made it env-configurable — [v2.35.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.35.0), [v2.35.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.35.1), [v2.36.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.0)
- v2.36.0 fixed "API and websocket authentication allowing refresh tokens" (#5387): refresh tokens are now only valid at `POST /auth/refresh`; using one as Bearer is rejected (`isBearerAccessTokenPayload` returns false for `type === 'refresh'`) — [v2.36.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.0); [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js)
- Bearer extraction: `Authorization: Bearer <jwt>` header OR `?token=<jwt>` URL query parameter (passport-jwt `fromUrlQueryParameter('token')`) — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js)
- Old non-expiring tokens are still accepted in 2.37.2: a JWT without `exp` passes, and the user is flagged `isOldToken` ("Temporary flag to report old tokens to users"). v2.26.1 shows a deprecation warning for the legacy token on the user page — [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js); [v2.26.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.26.1)
- Changing `JWT_SECRET_KEY` invalidates old tokens, so only migrated apps can authenticate — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460)
- Rate limiter on auth endpoints: `RATE_LIMIT_AUTH_WINDOW` default 10 min, `RATE_LIMIT_AUTH_MAX` default 40 attempts per IP (0 disables) — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460). `PATCH /api/me/password` is also behind this limiter — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)

API keys
- Admin-created API keys (`GET/POST/PATCH/DELETE /api/api-keys`) act on behalf of a chosen user, with optional expiry and active flag; used as `Authorization: Bearer <api_key>`. Expired keys are auto-deactivated on use — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460); [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js); [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js) (`jwt_payload.type === 'api'`)
- v2.37.0 (2026-09-28): "API keys can authenticate websocket connections" (#4974) — [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0)

Server discovery before login
- `GET /status` (no auth) returns `app`, `serverVersion`, `isInit`, `language`, `authMethods` (e.g. `local`, `openid`), `authFormData`. `GET /ping` and `GET /healthcheck` also exist — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js)
- `ROUTER_BASE_PATH` lets servers live under a subpath; v2.37.2 normalised it (leading slash, no trailing slash) — [v2.37.2](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.2)

OIDC / SSO for native apps
- Start: `GET /auth/openid` with mobile-flow params `response_type=code`, `redirect_uri=<app callback>`, `code_challenge=<S256>`, `code_challenge_method=S256` (only S256 accepted; challenge required in the mobile flow), optional `state`. The server validates `redirect_uri` against the admin setting `authOpenIDMobileRedirectURIs` (default `['audiobookshelf://oauth']`; a single entry `*` allows any URI), sets cookies (`auth_method=openid-mobile`) plus an express session, and 302-redirects to the IdP — [OidcAuthStrategy](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/OidcAuthStrategy.js); [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js); [ServerSettings](https://github.com/advplyr/audiobookshelf/blob/master/server/objects/settings/ServerSettings.js)
- The redirect URI registered at the IdP is always the server's own `<base>/auth/openid/mobile-redirect`; that endpoint looks up the stored app `redirect_uri` by `state` and redirects to `<app redirect_uri>?code=...&state=...` — [OidcAuthStrategy](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/OidcAuthStrategy.js) (`handleMobileRedirect`)
- Finish: client calls `GET /auth/openid/callback?code=...&state=...&code_verifier=...`. If the express session from the first request is missing, a mobile client gets `400 No session`. On success with `auth_method` `openid-mobile`, the server returns JSON (same payload as login, including the refresh token in the body) — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js) (`handleLoginSuccessBasedOnCookie`, `isAuthMethodAPIBased(['api','openid-mobile'])`)
- Desktop precedent: the Rust TUI Absotui implements SSO with a loopback listener at `http://127.0.0.1:41729/callback`, which the server admin must add to the allowed mobile redirect URIs; it offers a copy/paste fallback for headless use and says it "uses the same OpenID support as the Audiobookshelf mobile app" — [absotui README](https://github.com/pdwaldrop/absotui)
- Recent OIDC hardening: callback URL validation (v2.28.0), protocol validation (v2.30.0, v2.36.0 "Weak protocol validation for OIDC post-login callback URL"), cleanup of abandoned OIDC mobile login sessions (v2.37.1), OIDC callback cookie expiration extended to 10 minutes (v2.37.1). Web flow callbacks must be same-origin — [v2.28.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.28.0), [v2.30.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.30.0), [v2.36.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.0), [v2.37.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.1); [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js) (`isValidWebCallbackUrl`)
- The old API docs also describe OAuth2 SSO with PKCE (S256) for the Server section — [api.audiobookshelf.org](https://api.audiobookshelf.org/)

Multiple servers / users
- All API auth is stateless Bearer JWT per request; refresh-token sessions are stored per login in the server DB, so the same user can hold several sessions (one per device/client instance) — [Discussion 4460](https://github.com/advplyr/audiobookshelf/discussions/4460); [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js)
- Playback sessions are keyed by user + `deviceInfo.deviceId`: starting a session closes any open session for the same user and device. `deviceId` is client-supplied in `deviceInfo` (string fields: deviceId, clientVersion, manufacturer, model, sdkVersion, clientName, deviceName); if omitted the server generates one — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js); [DeviceInfo](https://github.com/advplyr/audiobookshelf/blob/master/server/objects/DeviceInfo.js)

### Inferences
- For a desktop app: always send `x-return-tokens: true` and `x-refresh-token`, never depend on cookies for API auth, and store tokens in the OS keyring (Windows Credential Manager / Secret Service), as Skald does.
- Implement a single-flight refresh per (server, user) account: rotation plus the 10-min grace period means concurrent refreshes with the same old token are tolerated for a while, but serialising them avoids the duplicate-token logout class of bugs fixed in 2.35.x.
- OIDC on desktop: a loopback `http://127.0.0.1:<port>/callback` redirect (RFC 8252 style) works only if the admin whitelists it; a custom URI scheme (e.g. `audiobookshelf://oauth`) would require OS-level protocol registration on Windows and a `.desktop` handler on Linux. Using `audiobookshelf://oauth` (the default whitelist entry) would collide with the official app's scheme, but needs no admin action. The `/auth/openid` request must be made by the app's own HTTP client (to capture cookies and the 302 Location), then the Location opened in the system browser, then `/auth/openid/callback` called with the same cookie jar. This is inferred from the code paths; confirm against the official app's implementation.
- Use a stable per-installation `deviceId` (distinct per account), otherwise two accounts or two servers on the same machine might close each other's sessions only if they share a user on the same server; for different servers there is no interaction.
- Treat `/status` `serverVersion` as a feature gate (e.g. < 2.26.0 means legacy token only; < 2.37.0 means API keys cannot auth sockets).

### Gaps
- Whether the legacy token path will actually be removed in a specific upcoming version: code in 2.37.2 still accepts it and no release note announces the removal date beyond "no earlier than September 30, 2025".
- The exact handling of `state` and `code_verifier` in the official Capacitor app was not read; flow described from server code only.

## Libraries, items, series, authors, collections, playlists, shelves, search, identifiers, dedup metadata

### Takeaway
Everything is under `/api/libraries/:id/...` with page/limit pagination, `sort`, `desc`, and a base64-encoded `filter` param. IDs are server-generated UUIDs that are unique per server only; cross-server dedup has to use metadata (`asin`, `isbn`, normalised title/author, series+sequence).

### Cited Findings
- Library endpoints: `GET /api/libraries`, `GET /api/libraries/:id`, `/items`, `/series`, `/series/:seriesId`, `/collections`, `/playlists`, `/personalized`, `/filterdata`, `/search`, `/stats`, `/authors`, `/narrators`, `/recent-episodes`, `/podcast-titles`, `/episode-downloads`, `/opml`, `/download` (multi-item zip) — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- `GET /api/libraries/:id/items` query params: `limit`, `page` (offset = page*limit), `sort`, `desc=1`, `filter`, `minified=1`, `collapseseries=1`, `include` (comma list). Response `{ results, total, limit, page, sortBy, sortDesc, filterBy, mediaType, minified, collapseseries, include }` — [LibraryController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryController.js) (`getLibraryItems`)
- Filter format is `<group>.<value>` where value is base64 then URI-encoded (server does `Buffer.from(decodeURIComponent(text), 'base64')`), e.g. `series.<b64 id>`, with special values like `no-series` — [LibraryController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryController.js); [libraryFilters.js](https://github.com/advplyr/audiobookshelf/blob/master/server/utils/queries/libraryFilters.js)
- Other query params seen in LibraryController: `q` (search), `ids`, `ext`, `force` — [LibraryController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryController.js)
- `/api/libraries/*` GETs go through `apiCacheManager.middleware` (server-side response cache) — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Items: `GET /api/items/:id` (supports expanded/include params), `POST /api/items/batch/get` (body list of ids), `GET /api/items/:id/cover`, `/download`, `/file/:fileid`, `/file/:fileid/download`, `/ebook/:fileid?`, `PATCH /api/items/:id/ebook/:fileid/status` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Series: `GET /api/series/:id` is marked `@deprecated`; use `/api/libraries/:id/series/:seriesId`. v2.37.0 fixed the library series endpoint "not checking the series belongs to that library" — [SeriesController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/SeriesController.js); [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0)
- Playlists: `GET /api/playlists` (all for user) is `@deprecated` in favour of `/api/libraries/:libraryId/playlists`; playlist items can be episodes (`DELETE /api/playlists/:id/item/:libraryItemId/:episodeId?`) — [PlaylistController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/PlaylistController.js); [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Collections (book-only, shared library-wide): `/api/collections`, `/api/collections/:id`, `/book`, `/batch/add`, `/batch/remove` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Authors: `GET /api/authors/:id`, `GET /api/authors/:id/image` (also unauthenticated, see downloads). v2.36.1 fixed "Author endpoints not checking user can access the author's library" — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js); [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1)
- `/api/search/*` (covers, books, podcast, authors, chapters, providers) are external metadata-provider searches, not library search; library search is `GET /api/libraries/:id/search?q=` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Continue listening / in progress: `GET /api/me/items-in-progress`, `GET /api/me/progress/:id/remove-from-continue-listening`, series hide/readd endpoints — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Book metadata fields available for dedup: `title`, `titleIgnorePrefix`, `subtitle`, `authors`, `narrators`, `series` (with sequence), `publishedYear`, `publisher`, `isbn`, `asin`, `language`, `explicit`, `abridged` — [Book model](https://github.com/advplyr/audiobookshelf/blob/master/server/models/Book.js)
- IDs are UUIDs (the server's CORS regex for item routes matches `[a-z0-9-]{36}`); v2.x migrated old non-UUID ids, and the server still resolves users by "id or old id" — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js); [TokenManager](https://github.com/advplyr/audiobookshelf/blob/master/server/auth/TokenManager.js) (`getUserByIdOrOldId`)
- Library items also carry a media id separate from the library item id; progress is keyed by `mediaItemId` (episode id or book media id) — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- v2.37.0 fixed "Server crash when collapsing sub-series and sorting by author name"; v2.36.1 fixed "Collapse series not working when filters are enabled" — [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0); [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1)

### Inferences
- A merged multi-server library needs a client-side key: prefer `asin`, then normalised `isbn` (ISBN-10/13 normalise), then normalised `titleIgnorePrefix` + primary author (+ series/sequence) fuzzy match. Item ids must be namespaced by server (e.g. `serverId:libraryItemId`).
- Progress is per server and user, so a merged view must pick a "winning" progress (e.g. latest `lastUpdate`) and decide whether to push it across servers. No server feature does this.
- Using `minified=1` with pagination is the cheap way to sync catalogs into a local DB; `/filterdata` gives authors/genres/tags/series/narrators lists for filter UIs.

### Gaps
- Exact list of valid `sort` keys and `include` values was not enumerated (they live in `libraryItemsBookFilters.js`/`libraryItemsPodcastFilters.js`).
- `/personalized` shelf ids (continue-listening, recently-added, discover, etc.) not enumerated from source here.
- No incremental "changed since" endpoint was found; delta sync seems to depend on sockets or refetching.

## Playback (play sessions, direct play vs HLS, tracks, chapters, sync, progress, local sessions, bookmarks)

### Takeaway
`POST /api/items/:id/play` (or `/play/:episodeId`) with `deviceInfo`, `mediaPlayer`, `supportedMimeTypes`, and optionally `forceDirectPlay`/`forceTranscode` returns a session with `audioTracks` (each with `contentUrl`, `startOffset`), chapters and `currentTime`. Sync with `POST /api/session/:id/sync` `{currentTime, timeListened, duration}` and close with `/close`. Offline listening is reported via `POST /api/session/local` or `/local-all`, where the server keeps whichever progress has the newer `updatedAt`.

### Cited Findings
- Endpoints: `POST /api/items/:id/play`, `POST /api/items/:id/play/:episodeId`, `GET /api/session/:id`, `POST /api/session/:id/sync`, `POST /api/session/:id/close`, `POST /api/session/local`, `POST /api/session/local-all`, `GET /api/sessions/open`, `GET /api/me/listening-sessions`, `GET /api/me/item/listening-sessions/:libraryItemId/:episodeId?` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Start options: `{ deviceInfo, forceDirectPlay?, forceTranscode?, mediaPlayer, supportedMimeTypes?: string[] }`. Direct play is chosen if `forceDirectPlay`, or if not `forceTranscode` and every included audio file's mimeType is in `supportedMimeTypes`; otherwise the server starts an ffmpeg HLS transcode (`Stream`) and returns one synthetic audio track — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js); [Book model](https://github.com/advplyr/audiobookshelf/blob/master/server/models/Book.js) (`checkCanDirectPlay`)
- Pitfall: if `supportedMimeTypes` is not an array, `checkCanDirectPlay` logs an error and returns false, so the server transcodes — [Book model](https://github.com/advplyr/audiobookshelf/blob/master/server/models/Book.js)
- Direct-play track `contentUrl` is `/api/items/{itemId}/file/{audioFile.ino}` (needs auth, so Bearer header or `?token=`) — [AudioTrack.js](https://github.com/advplyr/audiobookshelf/blob/master/server/objects/files/AudioTrack.js)
- HLS: served from `/hls/:stream/:file`; "HLS stream endpoint no longer requires user token in URL" (v2.22.0) and "Remove unnecessary token from HLS playlist files" (v2.29.0) — [HlsRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/HlsRouter.js); [v2.22.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.22.0); [v2.29.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.29.0)
- Public track route `GET /public/session/:id/track/:index` exists (no auth, by session id; used e.g. for casting); it redirects transcode requests to the HLS router — [PublicRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/PublicRouter.js); [SessionController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/SessionController.js)
- Starting a session for an item marked finished starts at 0 (progress reset); otherwise `startTime` is the stored `currentTime` — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- Starting a session closes any open session for the same user + device — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- Sync payload: `currentTime`, `timeListened` (added to session listening time), `duration` ("no longer required (v2.15.1) but used if available"). Sync emits `user_item_progress_updated` to the user's sockets — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- Open sessions not updated for 36 hours are closed as stale — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js) (`closeStaleOpenSessions`)
- Local session sync (`/api/session/local` single, `/api/session/local-all` with `{ sessions: [...], deviceInfo }`): validates `libraryItemId`, `libraryId`, `episodeId`; ids starting `play_local_` are mapped; progress is only overwritten if the existing progress `updatedAt` is older than the session's `updatedAt` ("Not updating progress ... because it has been updated more recently"). Result per session `{ id, success, progressSynced, error? }` — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- v2.26.0 fixed "Continue listening items not updating due to `updatedAt` timestamp not updating for mobile local syncs" — [v2.26.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.26.0)
- Progress API: `GET /api/me/progress` (all), `GET /api/me/progress/:id/:episodeId?`, `PATCH /api/me/progress/:libraryItemId/:episodeId?`, `PATCH /api/me/progress/batch/update`, `DELETE /api/me/progress/:id` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- MediaProgress fields: `currentTime`, `duration`, `progress`, `isFinished`, `hideFromContinueListening`, `ebookLocation`, `ebookProgress`, `finishedAt`, plus timestamps — [MediaProgress model](https://github.com/advplyr/audiobookshelf/blob/master/server/models/MediaProgress.js)
- Bookmarks: `GET /api/me/bookmarks`, `GET /api/me/bookmarks/:libraryItemId`, `POST/PATCH /api/me/item/:id/bookmark`, `DELETE /api/me/item/:id/bookmark/:time` (bookmarks are keyed by time) — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Chapters are part of the item media; edit via `POST /api/items/:id/chapters`; v2.36.1 stopped `PATCH /api/items/:id/media` from accepting `chapters`, `audioFiles`, `ebookFile` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js); [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1)
- Podcast episode track index: server accepts 0 or 1 for single-track episodes (old episodes pre-v2.21.0 have null index) — [SessionController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/SessionController.js)
- The official docs warn that third-party apps using non-recommended endpoints "could cause problems", and a search summary of the same page mentions wrong stats or listening sessions as the symptom — [ABS docs: Third-Party Apps](https://www.audiobookshelf.org/docs/documentation/community/community-apps)

### Inferences
- A native player (libmpv, GStreamer, LibVLC) can direct-play almost everything; send a generous `supportedMimeTypes` list (audio/mpeg, audio/mp4, audio/x-m4b, audio/aac, audio/ogg, audio/flac, audio/opus, audio/webm, audio/wav...) so the server never transcodes. Multi-file books need gapless concatenation using `startOffset` per track.
- Report progress with `/session/:id/sync` while online rather than raw `PATCH /me/progress`, so listening stats and sessions are recorded correctly; use `/session/local-all` for offline-buffered sessions. Sync interval of 10-30 s is typical (Skald uses 30 s).
- Clock skew matters for the local-sync `updatedAt` comparison; send timestamps based on a reliable clock.

### Gaps
- Full PlaybackSession JSON schema (fields like `displayTitle`, `chapters`, `mediaMetadata`) not copied verbatim; see `server/objects/PlaybackSession.js`.
- Whether `/api/session/:id/sync` returns 404 after a server restart (sessions are in memory) and how clients should recover was not verified.

## Downloads (whole item, single files, ebooks, covers) and auth on download URLs

### Takeaway
Whole item: `GET /api/items/:id/download` (zip, or the single file for single-file items). Single files: `/api/items/:id/file/:ino/download`. Both need the user's `canDownload` permission. Covers and author images are served without authentication.

### Cited Findings
- `GET /api/items/:id/download`: 403 unless `req.user.canDownload`; single-file items in a root dir are sent directly with the audio mime type, others are streamed as `<title>.zip` — [LibraryItemController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js)
- `GET /api/items/:id/file/:fileid` streams a library file (used for playback); `GET /api/items/:id/file/:fileid/download` forces download and also checks `canDownload` — [LibraryItemController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js)
- `GET /api/libraries/:id/download` downloads multiple items — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- Servers behind nginx may answer file requests with `204` + `X-Accel-Redirect` when configured — [LibraryItemController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js)
- Cover: `GET /api/items/:id/cover?width=&height=&format=&raw=&ts=`; format defaults to webp if the request accepts it else jpeg; width/height clamped to 4096; `ts` enables `Cache-Control: private, max-age=86400`. v2.36.1 restricted `format` to `webp`, `jpeg`, `png`; v2.37.0 requires valid id and positive width/height for cache endpoints — [LibraryItemController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js); [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1); [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0)
- Auth bypass list: GET `/api/items/:id/cover` and `/api/authors/:id/image` do not require auth — [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js) (`ignorePatterns`, `authNotNeeded`)
- Everything else under `/api` requires auth; for URLs handed to a player or image loader that cannot set headers, `?token=` works — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js); [Auth](https://github.com/advplyr/audiobookshelf/blob/master/server/Auth.js)
- Server sets `Referrer-Policy: no-referrer` "to protect against token exposure" — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js)

### Inferences
- Prefer HTTP Range-capable downloads of individual files (`/file/:ino/download`) over the zip, since the zip is streamed (no Content-Length, no resume). Per-file download also maps 1:1 to tracks for offline playback.
- Avoid `?token=` where possible; with 1-hour access tokens, long downloads or a player holding a URL can outlive the token. A native client can set headers on its own HTTP stack and on libmpv/GStreamer (`http-header-fields`).

### Gaps
- Whether `/download` endpoints support HTTP Range / resume was not verified (Express `res.download`/`sendFile` normally do, zip streaming does not).

## Ebooks

### Takeaway
Supported ebook types are `epub, pdf, mobi, azw3, cbr, cbz`. The file is served raw by `GET /api/items/:id/ebook/:fileid?`; rendering is the client's job. Reading position is stored in media progress as `ebookLocation` (string, e.g. EPUB CFI) and `ebookProgress` (float).

### Cited Findings
- `SupportedEbookTypes: ['epub', 'pdf', 'mobi', 'azw3', 'cbr', 'cbz']` — [globals.js](https://github.com/advplyr/audiobookshelf/blob/master/server/utils/globals.js)
- `GET /api/items/:id/ebook/:fileid?` serves the primary ebook or a given supplementary file; `PATCH /api/items/:id/ebook/:fileid/status` toggles primary/supplementary — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js); [LibraryItemController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/LibraryItemController.js)
- MediaProgress has `ebookLocation` (STRING) and `ebookProgress` (FLOAT) — [MediaProgress model](https://github.com/advplyr/audiobookshelf/blob/master/server/models/MediaProgress.js)
- The ebook and cover endpoints have special CORS handling because the Capacitor ereader fetches them via the fetch API — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js)
- Send-to-ereader: `POST /api/emails/send-ebook-to-device`; user ereader devices `POST /api/me/ereader-devices` — [ApiRouter](https://github.com/advplyr/audiobookshelf/blob/master/server/routers/ApiRouter.js)
- v2.36.1 hardened the comic book extractor path handling — [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1)

### Inferences
- Progress for ebooks is written through `PATCH /api/me/progress/:libraryItemId` with `ebookLocation`/`ebookProgress`. The location format is whatever the web reader writes (epub.js CFI for EPUB, page number for PDF/comics, per the web client), so interop with the web reader requires matching that format.

### Gaps
- Exact `ebookLocation` format used by the web client per format was not verified in client source.

## Realtime (Socket.IO)

### Takeaway
Connect Socket.IO to the server root, emit `auth` with the access token (or an API key since v2.37.0), wait for `init` or `auth_failed`. Key events for a player client: `user_item_progress_updated`, `user_updated`, `user_session_closed`, `item_updated`/`items_updated`/`item_added`/`item_removed`, library, series, collection and playlist events, `episode_added`.

### Cited Findings
- Client → server: `auth` (token), `ping` (server replies `pong`), `cancel_scan`, `search_covers`, `cancel_cover_search`, `set_log_listener`, `remove_log_listener`, `message_all_users` — [SocketAuthority](https://github.com/advplyr/audiobookshelf/blob/master/server/SocketAuthority.js)
- `auth_failed` messages include "Invalid token", "Token expired", "Invalid API key", "API key expired", "Invalid user"; on success the server emits `init` (admins also get `usersOnline`) — [SocketAuthority](https://github.com/advplyr/audiobookshelf/blob/master/server/SocketAuthority.js)
- Server → client event names found in server source (grep of emitter calls in 2.37.2): `user_item_progress_updated`, `user_session_closed`, `user_updated`, `user_added`, `user_removed`, `user_online`, `user_offline`, `user_stream_update`, `item_added`, `item_updated`, `item_removed`, `items_added`, `items_updated`, `episode_added`, `library_added`, `library_updated`, `library_removed`, `series_added`, `series_updated`, `series_removed`, `author_added`, `author_updated`, `author_removed`, `authors_num_books_updated`, `collection_added`, `collection_updated`, `collection_removed`, `playlist_added`, `playlist_updated`, `playlist_removed`, `stream_open`, `stream_ready`, `stream_progress`, `stream_closed`, `stream_error`, `stream_reset`, `rss_feed_open`, `rss_feed_closed`, `share_open`, `share_closed`, `episode_download_queued/started/finished/queue_cleared`, `task_started`, `task_progress`, `task_finished`, `track_started`, `track_progress`, `track_finished`, `metadata_embed_queue_update`, `batch_quickmatch_complete`, `notifications_updated`, `custom_metadata_provider_added/removed`, `backup_applied`, `admin_message` — [audiobookshelf server source](https://github.com/advplyr/audiobookshelf/tree/master/server)
- `user_item_progress_updated` is emitted via `clientEmitter(user.id, ...)` (only to that user's sockets) on session sync and local sync — [PlaybackSessionManager](https://github.com/advplyr/audiobookshelf/blob/master/server/managers/PlaybackSessionManager.js)
- Item events are filtered per user's library access (`libraryItemEmitter`) — [SocketAuthority](https://github.com/advplyr/audiobookshelf/blob/master/server/SocketAuthority.js)
- v2.36.0 closed a hole where refresh tokens could authenticate websockets — [v2.36.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.0)
- v2.36.1 fixed "Podcast rescan emitting stale episodes on `item_updated`" — [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1)

### Inferences
- With 1-hour access tokens, the socket must re-`auth` after refresh/reconnect; handle `auth_failed: Token expired` by refreshing and re-authing. Use one socket per (server, user) account.
- Socket.IO client libraries exist for Rust (`rust_socketio`, used by Skald), C++ (socket.io-client-cpp), .NET (SocketIOClient), Dart.

### Gaps
- Payload shapes of each event were not documented here; check the emitting call sites.

## API stability, versioning and documentation status

### Takeaway
There is no API versioning (no `/v1`, no deprecation headers); behaviour changes ship in minor releases with "API:" lines in release notes. api.audiobookshelf.org states it is out of date and unmaintained; the repo has a partial OpenAPI 3.0 spec under `docs/`. The server is moving to TypeScript (v2.37.0) and Node 24.

### Cited Findings
- api.audiobookshelf.org: "These API docs are out-of-date and are no longer maintained" and "We hope to implement automated OpenAPI docs in the future"; its sample login response shows server version 2.2.5 — [api.audiobookshelf.org](https://api.audiobookshelf.org/)
- The repo contains `docs/openapi.json` (OpenAPI 3.0.0, "Audiobookshelf API") with per-controller YAML for only some controllers (Author, Email, Library, Notification, Podcast, Series) — [docs folder](https://github.com/advplyr/audiobookshelf/tree/master/docs)
- Release cadence in 2026: v2.33.0 (Mar 12), v2.33.1, v2.33.2, v2.34.0 (Apr 27), v2.35.0 (May 17), v2.35.1, v2.36.0 (Jul 27), v2.36.1 (Sep 16), v2.37.0 (Sep 28), v2.37.1 (Sep 29), v2.37.2 (Oct 23) — [GitHub releases](https://github.com/advplyr/audiobookshelf/releases)
- Recent API-visible changes: settings PATCH accepts only a fixed set of general settings, cover formats restricted, media PATCH no longer accepts ebookFile/chapters/audioFiles, share endpoints now send 404 (v2.36.1); cover update path must be a file on the item, cover/author image cache endpoints validate id and size (v2.37.0); Node.js 24 required and initial TypeScript compilation (v2.37.0) — [v2.36.1](https://github.com/advplyr/audiobookshelf/releases/tag/v2.36.1); [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0)
- Deprecated in code: `GET /api/series/:id`, `GET /api/playlists` (use library-scoped), an old notifications route — [SeriesController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/SeriesController.js); [PlaylistController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/PlaylistController.js); [NotificationController](https://github.com/advplyr/audiobookshelf/blob/master/server/controllers/NotificationController.js)
- Official docs: "ABS is under constant development, and the API has ongoing updates" — [ABS docs: Third-Party Apps](https://www.audiobookshelf.org/docs/documentation/community/community-apps)
- CORS: browsers need `ALLOW_CORS=1` or the admin's `allowedOrigins`; ebook/cover routes allow `capacitor://localhost` and `http://localhost` by default — [Server](https://github.com/advplyr/audiobookshelf/blob/master/server/Server.js). (Not relevant for a native HTTP stack, but relevant for a WebView app that calls the API from JS, e.g. Tauri/Electron renderer fetch.)
- docs.audiobookshelf.org had a TLS certificate mismatch (Netlify cert) when fetched on 2026-10-24; www.audiobookshelf.org/docs worked — observed during this research.

### Inferences
- Build the client against `/status` `serverVersion` with feature flags and integration tests against several server versions (e.g. 2.26.x, 2.31.x, latest). Treat the server source as the spec.
- If the UI runs in a WebView, route API calls through the native side to avoid CORS configuration on users' servers (Skald does this via Rust `reqwest`).

### Gaps
- No public roadmap for a v3 / versioned API was found.

## Existing clients (official and third-party), desktop coverage, tech, gaps, multi-server support

### Takeaway
On Windows+Linux the options are Buchable/yaabsa (Flutter, by ABS contributor Vito0912), Skald (Tauri 2 + Rust + LibVLC, alpha, 5 stars), AudiobookOffline-Windows (PySide6, untested), and Rust TUIs (Toutui, Absotui; Linux/macOS). Multi-server account switching exists (Buchable, ShelfPlayer, EchoShelf), but only Skald merges libraries into one view, and that merges one server with local folders. No surveyed client merges libraries across several ABS servers or users.

### Cited Findings

Official
- Official mobile app `advplyr/audiobookshelf-app`: NuxtJS + Capacitor, Android (beta, Play Store) and iOS (TestFlight "early beta", "beta is full"), 2.8k stars, active (pushed 2026-10-11) — [audiobookshelf-app README](https://github.com/advplyr/audiobookshelf-app); GitHub API repo metadata
- Official web client is part of the server repo (Nuxt/Vue); there is no official desktop app. `mikiher/audiobookshelf-windows` installs and manages the ABS server on Windows with a tray app; it is not a player — [mikiher/audiobookshelf-windows](https://github.com/mikiher/audiobookshelf-windows)

Desktop (Windows/Linux)
- Buchable (`Vito0912/abs_flutter`): Dart/Flutter, Android, Windows, Linux, web (iOS/macOS untested). Features: "Fast Account Switching - Switch between multiple Audiobookshelf accounts with different servers in just two clicks", caching, pagination, queue (works offline), offline support on Windows and Linux. Gaps per its own matrix: media notifications not supported on Windows, ebooks very basic, socket support very basic, Linux "hasn't been tested". 219 stars, last push 2026-05-03. The author is working on "Buchable 2" — [abs_flutter README](https://github.com/Vito0912/abs_flutter)
- yaabsa (`Vito0912/yaabsa`, apparently the successor): Dart, Android/Android Auto/AAOS/Wear OS, iOS, macOS, Windows (.exe/.msix), Linux (Flatpak, AUR), web (unsupported). Highlights: eBook/PDF support with syncable annotations, subtitle/karaoke support, customisable player layout; discloses AI use in development. 98 stars, pushed 2026-10-23. Listed in official docs as "positioned as a full replacement for the ABS web app" — [yaabsa README](https://github.com/Vito0912/yaabsa); [ABS docs: Third-Party Apps](https://www.audiobookshelf.org/docs/documentation/community/community-apps)
- The same developer (Vito0912) authored several ABS server PRs, including the token-expiry change (#4756) and API-key websocket auth (#4974) — [v2.31.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.31.0); [v2.37.0](https://github.com/advplyr/audiobookshelf/releases/tag/v2.37.0)
- Skald (`Fireb-08/Skald`): Tauri 2 + React 19 + TypeScript + Rust (`reqwest`, `tokio`), audio via LibVLC (`vlc-rs`), Socket.IO via `rust_socketio`, tokens in OS keyring (`keyring` → Windows Credential Manager / Secret Service), local catalog SQLite (`rusqlite`). "Working alpha", Windows 11 x64 primary, Linux in development (Arch/CachyOS). Features: password or API-key login, gapless multi-file playback, waveform scrubber, chapters, EQ, bookmarks, Socket.IO progress sync every 30 s, offline downloads with later flush, server-free local libraries, admin tools, combined "All Libraries" shelf merging server and local libraries (v1.2.0). On Microsoft Store. Windows build needs bundled LibVLC DLLs plus `ffprobe.exe` and `tone.exe`. GPL-3.0, 5 stars — [Skald README](https://github.com/Fireb-08/Skald); [Microsoft Store listing](https://apps.microsoft.com/detail/9pcmr5kfshw1?hl=en-US&gl=US)
- Skald's README does not mention OIDC/SSO login or multiple servers — [Skald README](https://github.com/Fireb-08/Skald)
- AudiobookOffline-Windows (`shaynetroxler`): PySide6 (Qt), Windows; sister apps for macOS and Linux (GTK). Login, browse, search, series/collections, continue listening, stats, stream or download, chapters, speed 0.75-2x, sleep timer, progress reconciliation. Status: "core flow ... is implemented and untested against a real server"; SMTC/media keys incomplete. Single-exe release, unsigned. MIT, 1 star — [AudiobookOffline-Windows README](https://github.com/shaynetroxler/AudiobookOffline-Windows)
- Toutui (`AlbanDAVID/Toutui`): Rust TUI (ratatui), VLC playback, Linux and macOS, beta; progress sync, streaming; offline mode and playlists/collections listed as future features; stores token encrypted in SQLite. 171 stars, last push 2025-12-12 — [Toutui README](https://github.com/AlbanDAVID/Toutui)
- Absotui (`pdwaldrop/absotui`): Rust TUI, Linux and macOS, VLC or mpv, offline downloads with auto download, stats page, inline cover art (Kitty/Sixel/iTerm2), OIDC SSO via loopback callback, podcast subscription management. "Actively developed, single-maintainer". 27 stars, pushed 2026-10-24 — [absotui README](https://github.com/pdwaldrop/absotui)
- No Windows-native TUI or Qt/GTK client with real-world testing was found beyond the above.

Mobile / Apple (for feature reference)
- ShelfPlayer (iOS/iPadOS, Swift): repo archived; author sold the app to the Italian company Space Mushrooms at version 3.3.0 — [ShelfPlayer README](https://github.com/rasmuslos/ShelfPlayer). A web-search summary of its App Store page says it can connect to multiple servers or accounts simultaneously with "Listen Now" and universal search across all connected libraries — [App Store](https://apps.apple.com/app/apple-store/id6475221163) (via search snippet, not fetched directly)
- EchoShelf lists switching between multiple Audiobookshelf servers as a feature — [mwm.ai listing](https://mwm.ai/apps/echoshelf-audiobook-player/6763904592) (search snippet)
- Lissen (Android, Kotlin): minimalist; cloud sync, streaming, offline, Android Auto. 746 stars, pushed 2026-10-24 — [Lissen README](https://github.com/GrakovNe/lissen-android); [ABS docs: Third-Party Apps](https://www.audiobookshelf.org/docs/documentation/community/community-apps)
- Plappa (iOS, Swift/SwiftUI): client for Jellyfin and Audiobookshelf. 556 stars — [plappa repo](https://github.com/LeoKlaus/plappa)
- Official third-party list (Oct 2026) also includes Absorb, AudioBooth (iOS/macOS/watchOS), Auribook, Harmshelf (HarmonyOS incl. PC), SoundLeaf (iOS/macOS), Still, Storii (Flutter Android), Verbara, Voca. The only Windows/Linux entry is yaabsa; no CLI/TUI is listed. The page warns about "vibe-coded apps that are abandoned after a few weeks or months" and apps "using endpoints that are not recommended anymore" — [ABS docs: Third-Party Apps](https://www.audiobookshelf.org/docs/documentation/community/community-apps)

### Inferences
- Gap the new app can fill: a stable, low-footprint native (non-WebView or light WebView) Windows+Linux client with (a) true multi-server and multi-user accounts active at once, (b) a merged, deduplicated library view, (c) OIDC on desktop, (d) SMTC (Windows) and MPRIS (Linux) media controls, which Buchable lacks on Windows.
- Proven building blocks from Skald and the TUIs: LibVLC or mpv for playback, keyring for tokens, SQLite for a local catalog/offline queue, Socket.IO for live progress.
- Flutter (Buchable/yaabsa) is the most mature Windows+Linux GUI path so far; Tauri (Skald) is the main Rust path.

### Gaps
- No reported memory footprint numbers were found for any of these clients.
- Whether the official Capacitor app supports several saved servers/users at once was not verified from its source.
- yaabsa's multi-account support and OIDC support were not confirmed from its README excerpt.
- Desktop attempts that are abandoned (e.g. Electron wrappers of the web UI) were not surveyed in depth; the search surfaced only `benonymity/audiobookshelf` (a fork, purpose unknown).
