# Audio playback stacks for a native Windows + Linux Audiobookshelf player

Research date: 2026-10-24. Versions noted where sources gave them. Items marked "(unverified, prior knowledge)" come from the researcher's background knowledge and were not confirmed by a source fetched in this session; treat them as leads, not facts.

## 1. What formats and delivery modes does the player need to handle (incl. Audiobookshelf direct play vs HLS)?

### Takeaway
Audiobookshelf decides direct play vs HLS per session from the client-declared `supportedMimeTypes` (or `forceDirectPlay` / `forceTranscode`). Its HLS output is MPEG-TS segments of 6 s, with audio copied when possible and transcoded to AAC for FLAC/Opus/ALAC/WMA/etc. A backend that handles m4b/AAC, MP3, FLAC, Opus/Ogg via direct HTTP with a Bearer token avoids HLS in nearly every case.

### Cited findings
- Play-session options are `{forceDirectPlay?, forceTranscode?, mediaPlayer, supportedMimeTypes?}`; the decision is `shouldDirectPlay = options.forceDirectPlay || (!options.forceTranscode && libraryItem.media.checkCanDirectPlay(options.supportedMimeTypes, episodeId))`. Direct play returns the track list (`PlayMethod.DIRECTPLAY`); otherwise a `Stream` is created, a playlist generated and ffmpeg transcode started (`PlayMethod.TRANSCODE`) — [ABS PlaybackSessionManager.js](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/managers/PlaybackSessionManager.js)
- HLS details: client playlist at `/hls/{streamId}/output.m3u8`, `segmentLength = 6` (`-hls_time 6`), segment type `mpegts` (`output-%d.ts`; an fMP4 branch exists but is not used), audio codec `copy` by default, forced to `aac` for codecs `alac, ac3, eac3, opus` and MIME types including FLAC, OPUS, WMA, AIFF, WEBM, WEBMA, AWB, CAF; retries with AAC if copy fails — [ABS Stream.js](https://raw.githubusercontent.com/advplyr/audiobookshelf/master/server/objects/Stream.js)
- Auth: `Authorization: Bearer <token>`; for GET requests the token may also be passed as `?token=<token>`. Track `contentUrl` values are relative paths (example `/s/item/li_.../...mp3`). The API docs say they are out of date and no longer maintained — [Audiobookshelf API docs](https://api.audiobookshelf.org/)
- M4B (single file, embedded chapters) is the commonly preferred format; M4A, FLAC, OPUS, OGG also supported (third-party wiki, not official) — [serversatho.me wiki](https://wiki.serversatho.me/audiobookshelf)

### Inferences
- Since HLS from ABS is plain MPEG-TS with AAC or copied MP3/AAC, any backend built on FFmpeg (mpv, Qt Multimedia, libVLC) plays it out of the box. A Rust-native symphonia stack would need its own HLS client plus an MPEG-TS demuxer, which symphonia does not list. Recommendation: declare all MIME types the backend actually decodes and practically never request HLS; keep HLS as a fallback only if the backend supports it for free.
- The `?token=` query parameter is the simplest way to authenticate URLs handed to a player that cannot set headers; mpv can also set headers (see section 2 gaps).

### Gaps
- The `checkCanDirectPlay` implementation and the exact MIME list the official mobile app sends were not retrieved. Look at `server/models/Book.js` and the audiobookshelf-app repo.
- How `/hls/...` segment routes are authenticated was not confirmed from source.

## 2. Backend candidates: libmpv, GStreamer, FFmpeg, symphonia+rodio/cpal, Qt Multimedia, miniaudio, BASS, libVLC

### Takeaway
libmpv is the most proven choice for this use case (Supersonic, Feishin, Jellyfin Desktop) and covers every codec, HTTP/HLS, pitch-preserving speed, gapless and chapters, at the cost of shipping ~FFmpeg-sized DLLs on Windows (LGPL builds exist). Rust-native symphonia+rodio gives the smallest binary and permissive licensing but has gaps for this domain: no gapless on MP4/AAC, AAC-LC only natively, no HLS/TS, and you must build streaming, speed and seeking yourself. Qt 6.10+ Multimedia (FFmpeg backend, now with pitch compensation) is a strong option if the UI is Qt anyway. GStreamer is heavy on Windows.

### Cited findings

libmpv
- Feishin (GPL-3.0, ~10.2k stars, rewrite of Sonixd) has two backends: MPV and web audio; on first launch it asks for the path to an mpv binary (external process, not embedded libmpv); mpv 0.35.x/0.36.x work, 0.34.x broken — [Feishin GitHub](https://github.com/jeffvli/feishin)
- Supersonic (Go, Subsonic/Jellyfin client): "High-quality gapless audio playback powered by MPV, with optional audio exclusive mode"; MPRIS, Windows SMTC and macOS media center integration; ReplayGain (needs tags on server); Linux AppImage bundles mpv, the .tar.xz requires system libmpv (libmpv1 or libmpv2); Ubuntu build deps include `libmpv-dev` — [Supersonic GitHub](https://github.com/dweymouth/supersonic), [pkg.go.dev](https://beta.pkg.go.dev/github.com/dweymouth/supersonic@v0.8.0)
- Jellyfin Media Player was renamed Jellyfin Desktop, migrated Qt 5 to Qt 6, and added the MpvQt library dependency in 2.0.0; Flathub lists it as built with Qt WebEngine and libmpv — [linuxiac](https://linuxiac.com/?p=204889), [Flathub](https://flathub.org/sv/apps/org.jellyfin.JellyfinDesktop)
- libmpv licensing: mpv defaults to GPLv2+, but libmpv can be built LGPLv2.1+ with `-Dgpl=false` — [mpv embedding/building docs (mintlify mirror)](https://www.mintlify.com/mpv-player/mpv/embedding/building)
- An LGPL Windows libmpv build (emveepee/libmpv-mingw64-builder) ships `libmpv-2.dll`, `avcodec/avformat/avutil/avfilter`, `swresample`, `swscale` (all LGPLv2.1+), and `libplacebo` (separate DLL in v0.40.0/0.41.0 zips, static in 0.37–0.39); FFmpeg built without `--enable-gpl`, no x264/x265, no OpenSSL (TLS via Windows SChannel); dynamic linking + import libs satisfy relinking — [libmpv-mingw64-builder License wiki](https://github.com/emveepee/libmpv-mingw64-builder/wiki/License)
- shinchiro's mpv-dev Windows archives contain `libmpv-2.dll`; builds retained 30 days; license status of that build not stated — [mpv-winbuild fork README](https://github-redirect.dependabot.com/dyphire/mpv-winbuild)
- Rust binding `libmpv2` 6.0.0 (2026-05-12), maintained by kohsine; minimum libmpv API 2.0 (mpv 0.35.0); `build_libmpv` feature eases Windows cross-compiling; Windows needs prebuilt artifacts in `MPV_SOURCE/64`; docs coverage 31% — [docs.rs libmpv2](https://docs.rs/crate/libmpv2/latest)
- Go IPC-based mpv control libraries exist (blang/mpv) as an alternative to embedding — [blang/mpv](https://awesome.ecosyste.ms/projects/github.com%2Fblang%2Fmpv)

symphonia + rodio/cpal (Rust-native)
- symphonia 0.6.1 (2026-08-13), MPL-2.0. AAC-LC "Great", not gapless, feature `aac`, not default; ISO/MP4 demuxer "Great", not gapless, feature `isomp4`, not default; ALAC "Great" gapless; MP3 "Excellent" gapless; FLAC "Excellent" gapless; Vorbis "Excellent"; Opus now listed (feature `opus`, default, gapless). HE-AAC/HE-AACv2 only via third-party `symphonia-adapter-fdk-aac` (libfdk-aac); third-party adapters explicitly not endorsed. MSRV for 0.6.x is 1.85. Docs say nothing about seeking accuracy or chapters — [Symphonia GitHub](https://github.com/pdeljanov/Symphonia), [docs.rs symphonia](https://docs.rs/crate/symphonia/latest)
- rodio 0.23.0 (2026-10-11), depends on cpal ^0.18.2; symphonia is the default decoder, with optional claxon/minimp3/lewton/hound; on Linux needs ALSA dev files — [docs.rs rodio](https://docs.rs/crate/rodio/latest)
- `stream-download` 0.24.4 (2026-08-29), MIT/Apache-2.0: turns a remote HTTP stream into a seekable `Read + Seek` source backed by temp-file, memory, bounded or adaptive storage; seeking ahead of the download restarts the stream at the requested position; reqwest by default with rustls/native-tls choice; per-request customization is the route for auth headers; reqwest-middleware for retries — [docs.rs stream-download](https://docs.rs/crate/stream-download/latest)
- No search result showed a Symphonia-specific m4b seeking bug; nothing positive about long-file seek accuracy either — [search, mintlify symphonia formats](https://www.mintlify.com/pdeljanov/Symphonia/resources/supported-formats)

Qt Multimedia
- Qt 6.12 docs: FFmpeg is the default backend on all platforms except WebAssembly and embedded Linux/Boot2Qt; Windows Media Foundation backend deprecated since 6.10; GStreamer only recommended for embedded. Qt 6.12 bundles FFmpeg 9.0.1 (LGPLv2.1+) and lists Signalsmith Stretch 1.1.0 (MIT) as a third-party module. Online-installer FFmpeg binaries are dynamically linked and contain only permissively licensed features. Qt Multimedia is commercial, LGPLv3 or GPLv2 — [Qt Multimedia 6.12](https://doc.qt.io/qt-6/qtmultimedia-index.html)
- `MediaPlayer.pitchCompensation` added in Qt 6.10, on by default where supported; `pitchCompensationAvailability` = AlwaysOn / Available / Unavailable per backend; `playbackOptions` changes apply on next source change — [Qt MediaPlayer QML](https://doc.qt.io/qt-6/qml-qtmultimedia-mediaplayer.html), [QMediaPlayer 6.10](https://doc.qt.io/qt-6.10/qmediaplayer.html)

GStreamer
- Latest stable 1.28.6; MSVC recommended on Windows; installers built with Inno Setup 6 since 1.28 (user-only installs possible) — [GStreamer download](https://gstreamer.freedesktop.org/download/)
- Older docs: runtime needs ~340–350 MB free disk (as of ~1.15); official deployment guide warns full-runtime bundling "installs unnecessary files", offers private install via `INSTALLDIR`, or hand-picking libraries; plug-ins load per media type, so you must know which DLLs you need — [GStreamer Windows deployment](https://gstreamer.freedesktop.org/documentation/deploying/windows.html), [Installing on Windows](https://gstreamer.freedesktop.org/documentation/installing/on-windows.html), [fyne-streamer wiki](https://github.com/metal3d/fyne-streamer/wiki/Installation-on-Windows)
- 1.24.3 MSVC runtime had an empty qt6 .msm — [cerbero issue #480](https://gitlab.freedesktop.org/gstreamer/cerbero/-/issues/480)

### Inferences
- Size ranking (rough): symphonia+rodio (a few MB in the executable) < miniaudio (single C file; MP3/FLAC/WAV decoders built in, no AAC) < libmpv LGPL bundle (FFmpeg DLLs dominate; on the order of tens of MB, can be cut with an audio-only FFmpeg build) ~ Qt Multimedia FFmpeg < libVLC (plugins directory) < GStreamer full runtime (hundreds of MB unless hand-trimmed). Exact numbers were not found; measure.
- On Linux, every FFmpeg/mpv/GStreamer option can rely on distro packages (or Flatpak runtime), so size matters mostly for the Windows installer.
- For a Rust app, the lowest-effort full-featured path is libmpv via `libmpv2` (or a thin own FFI over the small `client.h` API), with an audio-only LGPL libmpv build shipped on Windows. The Rust-native path is viable only if accepting: AAC-LC only (HE-AAC needs libfdk-aac, which has its own non-GPL-compatible license), no MP4 gapless, no HLS, and self-built speed/streaming/seek handling.
- BASS (un4seen) is closed-source and needs a paid licence for commercial/shareware use; free for non-commercial (unverified, prior knowledge). It would conflict with a FOSS release.
- libVLC (LGPLv2.1+) has the same codec coverage as mpv, but its Windows plugin tree is larger and audio-only apps rarely choose it (unverified, prior knowledge).
- Writing directly against FFmpeg (libavformat/libavcodec + own output via cpal/miniaudio) gives control and the same codec coverage as mpv but means reimplementing buffering, seeking, gapless and tempo. libmpv already does that.

### Gaps
- No measured on-disk size of a current audio-only LGPL libmpv bundle, Qt Multimedia FFmpeg plugin, or libVLC on Windows was found.
- Symphonia seek accuracy on 10–40 h m4b files and AAC priming/edit-list handling was not documented; needs a test with real files.
- miniaudio, BASS and libVLC current versions/licensing were not fetched this session.
- Whether Qt's FFmpeg backend on Linux uses system FFmpeg or bundled, and whether it exposes HTTP headers for auth, not confirmed.

## 3. Variable speed with pitch preservation

### Takeaway
mpv (scaletempo2 by default, optional rubberband), Qt 6.10+ (pitchCompensation, apparently via bundled Signalsmith Stretch) and GStreamer (scaletempo) all provide it built in. On a Rust-native stack you add a stretcher yourself: `signalsmith-stretch` / `ssstretch` bindings (C++ library, MIT) or pure-Rust `timestretch` / `wsola`.

### Cited findings
- mpv: pitch correction is on by default; speed above normal auto-inserts the `scaletempo2` filter; `--pitch` also uses scaletempo2 — [mpv manual](https://mpv.io/manual/master/)
- Qt 6.10 `pitchCompensation` property, backend-dependent availability — [Qt MediaPlayer QML](https://doc.qt.io/qt-6/qml-qtmultimedia-mediaplayer.html); Qt 6.12 ships Signalsmith Stretch 1.1.0 (MIT) as a third-party module — [Qt Multimedia 6.12](https://doc.qt.io/qt-6/qtmultimedia-index.html)
- GStreamer scaletempo (gst-plugins-good): when a seek event has rate != 1.0, it scales tempo without pitch; placed before the audio sink, e.g. via playbin's audio-sink — [gstscaletempo.c](https://git.stg.centos.org/source-git/gstreamer1-plugins-good/blob/c8s/f/gst/audiofx/gstscaletempo.c)
- Rust: `signalsmith-stretch` (wrapper of the C++11 Signalsmith Stretch, real-time capable, reports input/output latency); `ssstretch` 0.1.0 (another binding); `timestretch` (WSOLA + phase vocoder hybrid, v0.9.x); `wsola` (time-domain, real-time, good on speech) — [docs.rs signalsmith-stretch](https://docs.rs/signalsmith-stretch), [docs.rs ssstretch](https://docs.rs/ssstretch), [docs.rs timestretch](https://docs.rs/timestretch), [docs.rs wsola](https://docs.rs/wsola)
- Cozy offers per-book speed up to 2x and depends on GStreamer; the exact element used was not confirmed — [linuxuprising](https://linuxuprising.com/2018/09/cozy-is-nice-linux-audiobook-player-for.html), [Debian cozy](https://packages.debian.org/nl/forky/cozy)

### Inferences
- For speech at 0.5–3x, WSOLA-type algorithms (scaletempo2, wsola) are adequate and cheap; Signalsmith/rubberband give better quality for music but are not needed for audiobooks.
- rodio's built-in speed control resamples (changes pitch) (unverified, prior knowledge), so a stretcher Source wrapper is required on that stack.

### Gaps
- `af=rubberband` availability in LGPL libmpv builds (rubberband is GPL) not confirmed; assume unavailable in LGPL builds.
- Which Qt FFmpeg backend implementation uses Signalsmith for pitchCompensation is inferred from the third-party list, not stated.

## 4. Gapless multi-file books, accurate seeking in long m4b, sleep timer, normalization/boost

### Takeaway
mpv provides gapless audio across playlist entries (used by Supersonic), demuxer caching, chapter navigation and filters (loudnorm/dynaudnorm via FFmpeg lavfi). Symphonia has no gapless for MP4/AAC. Sleep timer is app logic on any backend.

### Cited findings
- Supersonic markets "gapless audio playback powered by MPV" — [Supersonic GitHub](https://github.com/dweymouth/supersonic)
- Symphonia: gapless requires both demuxer and decoder support; isomp4 and AAC are both marked not gapless — [Symphonia GitHub](https://github.com/pdeljanov/Symphonia)
- mpv caching: `demuxer-cache-duration`/`demuxer-cache-state` properties, `--cache=yes`, `--demuxer-max-bytes`, `--demuxer-readahead-secs`, `--demuxer-cache-wait`; chapter seek keys and `--chapter-seek-threshold` (default 5 s) — [mpv manual](https://mpv.io/manual/master/)
- stream-download restarts the HTTP stream at the seek position when seeking past downloaded data — [docs.rs stream-download](https://docs.rs/crate/stream-download/latest)

### Inferences
- For audiobooks, true sample-accurate gapless matters less than for music; multi-file books are usually split at chapter boundaries. A short gap is tolerable, but mpv's `--gapless-audio` and `--prefetch-playlist` (unverified option names, prior knowledge) handle it if wanted.
- Volume boost: mpv's `volume-max` allows >100% (unverified, prior knowledge); with rodio/cpal apply gain plus a limiter in your Source chain.
- Long-file seeking in MP4 depends on the sample table (stts/stsz/stco) being parsed; FFmpeg-based backends are mature here. Test symphonia on a 40 h m4b before committing.

### Gaps
- mpv manual sections for `--gapless-audio`, `--prefetch-playlist`, `--http-header-fields`, `volume-max`, loudnorm were not reached in the fetched portion (page truncated at 100k of 647k chars).
- No reports found on seek latency in 40 h m4b for any backend.

## 5. OS media controls (MPRIS, SMTC), media keys, notifications, tray

### Takeaway
`souvlaki` (MIT) is the standard Rust crate: MPRIS on Linux (dbus-crossroads default or pure-Rust zbus), Windows media controls via SMTC, requires an HWND on Windows. A 0.9 rewrite is in progress. Supersonic shows the same feature set (MPRIS + SMTC) in Go.

### Cited findings
- souvlaki: one API for Linux/BSD (MPRIS), macOS/iOS, Windows; Linux default backend `dbus-crossroads` (native libdbus, noted as having "some issues with consistency"), alternative `zbus` backend (pure Rust, `use_zbus` feature, own thread with pollster); Windows needs an open window's `HWND` (via raw_window_handle/winit); 0.9 rewrite on a branch, pre-release planned around October 2026; 16 open issues; MSRV 1.67; MIT — [souvlaki GitHub](https://github.com/Sinono3/souvlaki), [docs.rs souvlaki 0.8.0](https://docs.rs/crate/souvlaki/0.8.0)
- Supersonic: "MPRIS, Windows SMTC, and Mac OS media center integration for media key and desktop control" — [Supersonic GitHub](https://github.com/dweymouth/supersonic)

### Inferences
- Direct alternatives: on Windows the `windows` crate exposes `Windows.Media.SystemMediaTransportControls` (via `ISystemMediaTransportControlsInterop::GetForWindow`); on Linux `mpris-server` (zbus) or Qt's own D-Bus. These avoid a dependency on souvlaki's release cadence (unverified, prior knowledge).
- With SMTC/MPRIS in place, hardware media keys work through the OS; a separate global hotkey crate is only needed for custom shortcuts.
- Tray/notifications: `tray-icon` and `notify-rust` are common Rust choices (unverified, prior knowledge, not researched this session).

### Gaps
- Tray and notification libraries were not researched with sources.
- Which apps depend on souvlaki was not listed on its page (spotify-player and others are believed to use it, unverified).

## 6. Local chapter metadata extraction for offline files

### Takeaway
For m4b, chapters live in either a QuickTime chapter text track or the Nero `chpl` atom; `mp4ameta` reads both. ffprobe (or libmpv's `chapter-list` property) covers all containers when FFmpeg is already shipped. Simplest: store the chapter list ABS returns with the item at download time and only fall back to local parsing.

### Cited findings
- mp4ameta: `tag.chapter_track()` (QuickTime-style track) and `tag.chapter_list_mut()` (Nero chpl) are separate storage locations; has a `ChplTimescale` read/write setting — [docs.rs mp4ameta](https://docs.rs/mp4ameta)
- Nero chapters are atom `moov.udta.chpl`, start times in 100 ns units with length-prefixed titles; TagLib caps Nero chapter count at 255 — [TagLib NeroChapterList](https://taglib.org/api/classTagLib_1_1MP4_1_1NeroChapterList.html)
- iTunes uses QT chapters and ignores Nero; "pretty much everything else" uses Nero if present (user report) — [mp3tag community](https://community.mp3tag.de/t/67243)

### Inferences
- lofty reads tags/cover art broadly but chapter support for MP4 was not confirmed (unverified); mp4ameta is the targeted crate for m4b chapters. MP3 chapters (ID3 CHAP) can be read with the `id3` crate (unverified).
- Because ABS already computes chapters server-side, persisting its JSON alongside the downloaded files avoids most local parsing and handles multi-file books (where chapters span files).

### Gaps
- lofty chapter support and the ABS chapter JSON schema were not fetched.

## 7. How existing open-source players do it, and footprints

### Takeaway
mpv is the backend of choice for the server-client players most like this project (Supersonic, Feishin, Jellyfin Desktop). GStreamer dominates GNOME-native players (Cozy). Rust music players typically use rodio/cpal or custom symphonia pipelines.

### Cited findings
- Supersonic: Go + libmpv, bundled mpv on Windows and in AppImage, system libmpv for tarball — [Supersonic GitHub](https://github.com/dweymouth/supersonic)
- Feishin: Electron + external mpv binary or web audio — [Feishin GitHub](https://github.com/jeffvli/feishin)
- Jellyfin Desktop: Qt 6 + libmpv via MpvQt; does not support Ubuntu 24.04 because its Qt stack is 6.4 — [linuxiac](https://linuxiac.com/?p=204889)
- Cozy: Linux audiobook player using GStreamer, per-book speed up to 2x — [Debian cozy](https://packages.debian.org/nl/forky/cozy), [linuxuprising](https://linuxuprising.com/2018/09/cozy-is-nice-linux-audiobook-player-for.html)
- The "reel" project has a Rust MPV backend — [reel v0.2.0 release](https://newreleases.io/project/github/arsfeld/reel/release/v0.2.0)

### Inferences (unverified, prior knowledge)
- Amberol (GTK/Rust) uses gstreamer-rs; psst uses its own symphonia + cpal pipeline; spotify-player uses librespot with rodio sink; Strawberry (Qt/C++) uses GStreamer; Tauon uses its own C backend (phazor) built on miniaudio-type code; Finamp is Flutter (just_audio / media_kit). These were not verified this session.
- Memory: libmpv audio-only typically tens of MB RSS including demuxer cache (configurable via `demuxer-max-bytes`); rodio/symphonia a few MB plus buffers. No measured figures were found.

### Gaps
- No sourced memory or binary size measurements for any backend.
- Source code of Amberol, psst, spotify-player, Tauon, Strawberry, Finamp was not inspected.

## Overall recommendation (inference)
1. Default: libmpv (LGPL, audio-only build on Windows, system libmpv on Linux/Flatpak), driven from Rust via `libmpv2` or C++/Qt via MpvQt. Covers all formats, HTTP with token/headers, HLS fallback, scaletempo2 speed, chapters, caching. Pair with souvlaki (or direct SMTC/MPRIS code).
2. If the UI is Qt 6.10+: Qt Multimedia's FFmpeg backend is a reasonable alternative with built-in pitch compensation and no extra dependency, though less control than mpv.
3. Rust-native (symphonia 0.6 + rodio 0.23 + stream-download + a WSOLA/Signalsmith stretcher): smallest and fully permissive, but carries AAC-LC-only, no MP4 gapless, no HLS, and more code to own. Prototype with a 40 h m4b before choosing it.
4. Avoid GStreamer on Windows (size, deployment) and BASS (licensing) for this project.
