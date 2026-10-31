# 0006: Local folders

Status: accepted

## Decision

Folders on this computer are stored as one built-in account ("This computer", `account.local = 1`) that never connects. Each folder is a library and each book an item whose `raw_json` has the shape of an ABS library item. The tables `local_folder`, `local_book` and `local_track` hold what only local books have. Books are recognised offline with the rules of the ABS scanner, without online lookups.

## Reasons

- Catalog, search, home rows, progress, the "All accounts" page and works are all keyed by account and item, so local books work there without separate code. A local book and a server's copy of it become one work.
- Tags and chapters come from `lofty`, `mp4ameta` and `id3`, which read headers only. Opening a long M4B through libmpv costs about 110 MB (`docs/spike-findings.md`) and would make `abs-core` depend on libmpv. The 40 h fixture with 80 chapters reads in milliseconds.

## Differences from ABS

ABS assumes a managed library. Here people pick any folder, so loose files are handled differently (`crates/core/src/local/group.rs`):

- Loose MP3s directly in the chosen folder are one book, or one per album tag. In ABS each file is a book.
- Several M4B or M4A in one folder with different albums or names are separate books. In ABS they are one book.
- Several differently named ebooks without audio in one folder are separate books. In ABS only one of them is used.
- A loose `Dune.m4b` and `Dune.epub` with the same file stem are one book.

Not ported: `.nfo` files, audio formats `lofty` cannot read (WMA, MKA, WebM, CAF, AWB, MPEG), and the online Match feature.

## Consequences

- The app never writes to a chosen folder.
- A folder that cannot be read, or that has no media left while books are known, counts as unreachable and keeps its books, so an unplugged drive loses nothing. Deleting every book from a folder therefore needs "Remove".
- A book's id comes from the folder id and its relative path. A book that moves gets a new id; a new book with the same file names and sizes takes over its progress and bookmarks.
- No file watcher. Folders are scanned at start, on "Sync now", on "Scan again" and at the sync interval.
- Listening on a local book records no `local_session`, because nothing could send it. A book counts as finished 10 s before its end, the ABS default.
