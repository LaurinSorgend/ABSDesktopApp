# 0002: UI, playback and reader stack

Status: proposed. Windows numbers are in; Linux (PSS from `/proc/<pid>/smaps_rollup`, GNOME and KDE on Wayland) is still to be measured before this is accepted.

## Measurements

Windows 11, release builds, private working set. Details and method in `docs/spike-findings.md`.

| Scenario | Spike | Result |
|---|---|---|
| Main window idle, first screen of covers | `cover_grid`, FemtoVG | 89.4 MB (software renderer: 26.5 MB) |
| Browsing 500 covers, 64 MB decoded cache full | `cover_grid` + `SPIKE_AUTOSCROLL` | 139.9 MB peak (software: 83.2 MB) |
| libmpv idle handle | `mpv_idle` | 6.8 MB |
| Playing an MP3 track, demuxer cache capped at 8 MiB | `mpv_idle` | 24.5 MB |
| Playing a 40 h single-file M4B, cache capped | `mpv_idle` | 133.9 MB |
| Reader process tree, EPUB open | `abs-reader` | 80.7 MB over 7 processes |

## Decision

Slint: go. The cover grid stays within budget with the default FemtoVG renderer, and the software renderer is a fallback that saves about 60 MB if GPU memory turns out to matter more than smoothness. Scrolling and rendering on Windows 11 were checked by hand and look good; fractional scaling on Wayland is still open.

libmpv: go, with conditions:

- The demuxer cache must be capped (`demuxer-max-bytes`, `demuxer-max-back-bytes`); the default 150 MiB forward cache tripled memory for an MP3 track.
- mpv's own Windows media controls (`media-controls`) are turned off so they don't compete with souvlaki.
- Long single-file M4B books cost about 110 MB extra because FFmpeg keeps the MP4 sample index in memory. Before Phase 1.7, measure the symphonia fallback on the same file. If it is much lower, prefer symphonia for M4B; otherwise accept the cost.
- Releases need an LGPL libmpv build. The development build from shinchiro is GPL and 116 MB.

foliate-js in a wry helper process: go. EPUB, CBZ and PDF open, CFIs round-trip, and the whole process tree goes away when the reader closes.

## Provisional memory budgets (Windows, private working set)

| State | Budget |
|---|---|
| Idle main window | 90 MB |
| Browsing a large library | 150 MB |
| Playing (added to the above) | 40 MB; 140 MB for a long single-file M4B until the symphonia comparison is done |
| Reader helper tree | 110 MB |

Linux budgets get set once the Linux numbers exist.
