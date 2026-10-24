# Ebook reader inside a native Windows + Linux Audiobookshelf desktop app

Scope: EPUB (primary), PDF, CBZ/CBR, MOBI/AZW3 for a non-Electron desktop client. Research date 2026-10-24. Fewer tool calls than a full survey; gaps are listed per section.

## 1. How Audiobookshelf stores ebook progress, and how much CFI compatibility matters

### Takeaway
The ABS web reader uses epub.js. It PATCHes `ebookLocation` (the epub.js CFI string for the start of the current view) and `ebookProgress` (a 0..1 percentage for the end of the view) to `/api/me/progress/{libraryItemId}`. A desktop reader that writes valid EPUB CFIs will round-trip with the web reader. A reader that only writes a percentage still updates the progress bar, but the web reader will open the book at the start.

### Cited Findings
- `EpubReader.vue` imports `ePub from 'epubjs'` and, in the `relocated` handler, calls `updateProgress` unless `location.start.cfi` equals the saved location — [ABS EpubReader.vue](https://github.com/advplyr/audiobookshelf/blob/master/client/components/readers/EpubReader.vue)
- The payload is `{ ebookLocation: location.start.cfi, ebookProgress: location.end.percentage }`, sent with `this.$axios.$patch('/api/me/progress/${libraryItemId}', payload)`. `ebookProgress` is omitted when the percentage is falsy — [ABS EpubReader.vue](https://github.com/advplyr/audiobookshelf/blob/master/client/components/readers/EpubReader.vue)
- On load it calls `rendition.display(savedEbookLocation || book.locations.start)`. The restore path only accepts values beginning with `epubcfi` — [ABS EpubReader.vue](https://github.com/advplyr/audiobookshelf/blob/master/client/components/readers/EpubReader.vue)
- Percentages come from `book.locations.generate()` plus `percentageFromCfi()`. Generated locations are cached in browser localStorage under `ebookLocations-${libraryItemId}`, capped at 3,000,000 bytes with LRU eviction. This cache is client-side only and is not sent to the server — [ABS EpubReader.vue](https://github.com/advplyr/audiobookshelf/blob/master/client/components/readers/EpubReader.vue)
- foliate-js ships `epubcfi.js`, which parses and stringifies EPUB CFIs, including range CFIs, and accepts a filter so injected nodes are ignored — [foliate-js README](https://github.com/johnfactotum/foliate-js)
- Third-party ABS clients already advertise cross-device ebook progress sync, for example "Still" on iOS (EPUB/PDF/CBZ) — [App Store: Still for Audiobookshelf](https://apps.apple.com/il/app/id6754208326). The community abs-kosync-bridge connects ABS with KOReader/KoSync progress — [abs-kosync-bridge](https://github.com/J-Lich/abs-kosync-bridge)

### Inferences
- The CFI ABS stores is epub.js's `location.start.cfi`, typically `epubcfi(/6/N[idref]!/4/.../x:offset)`. A spine-step plus element-path CFI is a standard format, so foliate-js output should parse in epub.js and the reverse, as long as both point at the same spine item and element. Character offsets can resolve to a slightly different spot after reflow, which is acceptable.
- For a native (non-JS) engine, the minimum you need for compatibility is: (1) parse the spine index and element path of an incoming CFI to jump to the right chapter and element, and (2) emit a CFI built from spine index plus element child indices for the first visible element. Emitting chapter-level CFIs only (`/6/N!/4`) still restores the right chapter in the web reader.
- `ebookProgress` semantics differ between engines (epub.js derives it from generated locations, roughly every 150 chars by default). A native engine can compute its own percentage, such as characters read over total characters. The ABS progress bar will show a slightly different value from each client, which does not matter for a simple reader.
- PDF progress in ABS was not checked; it probably stores a page number in `ebookLocation`. Verify in `PdfReader.vue` (see Gaps).

### Gaps
- Did not inspect `PdfReader.vue`, `ComicReader.vue`, or the official mobile app (audiobookshelf-app) to confirm what they write into `ebookLocation` for PDF/CBZ, or whether mobile uses the same epub.js CFI.
- Could not confirm the epub.js version bundled with the ABS client.
- Found no Rust or C++ library dedicated to generating epub.js-compatible CFIs from a non-browser DOM. This would need a small custom implementation (CFI grammar is in the EPUB CFI spec).

## 2. EPUB approach (a): embedded system webview + JS reader (foliate-js / epub.js)

### Takeaway
This approach gives the best EPUB fidelity and the least code. foliate-js (MIT) covers EPUB, MOBI, AZW3, FB2, CBZ and has CFI built in. wry can embed a WebView2/WebKitGTK child into a non-webview Rust window. The cost is a second rendering runtime with its own memory (WebView2 is Chromium-class, so expect 100+ MB) and the webview-in-native-window integration work, which is weakest on Linux Wayland.

### Cited Findings
- foliate-js: "Library for rendering e-books in the browser", MIT. Formats: EPUB, MOBI, KF8 (AZW3), FB2, CBZ. PDF is experimental and needs PDF.js — [foliate-js](https://github.com/johnfactotum/foliate-js)
- foliate-js has no hard dependencies and uses native ES modules with no build step. zip.js is recommended for archives and fflate for KF8 fonts. Font deobfuscation uses Web Crypto and needs a secure context — [foliate-js](https://github.com/johnfactotum/foliate-js)
- Its paginator uses CSS multi-column layout (same strategy as epub.js) and supports paginated and scrolled modes, but not continuous scrolling. The README calls it slow and notes CSS limitations — [foliate-js](https://github.com/johnfactotum/foliate-js)
- Status: "not stable", the API may change, and there is no formal release. The maintainers recommend using it as a git submodule. At the time of fetching: 299 commits, 38 open issues, 30 open PRs — [foliate-js](https://github.com/johnfactotum/foliate-js)
- Memory: books are not fully loaded into memory and images load on demand. MOBI is decompressed at once but split into sections. KF8 decompression and search are slow — [foliate-js](https://github.com/johnfactotum/foliate-js)
- Security: scripted EPUBs are unsupported and the README says CSP is essential — [foliate-js](https://github.com/johnfactotum/foliate-js)
- wry 0.35 changed constructors to take a `RawWindowHandle` and added child-webview constructors. It also added the Windows-only `set_memory_usage_level`, which can "significantly reduce memory consumption" when set to Low while the app is inactive — [wry 0.35.0 release notes](https://v2.tauri.app/release/wry/v0.35.0/)
- Current wry (0.56/0.57 on docs.rs): `build_as_child` works on Windows and on Linux under X11 only. On Linux, WebKitGTK needs GTK, so winit-based apps must call `gtk::init`. For Wayland the docs recommend `build_gtk` with a `gtk::Fixed` container — [wry docs.rs 0.56.0 README](https://docs.rs/crate/wry/0.56.0/source/README.md), [wry 0.57.0](https://docs.rs/crate/wry/0.57.0)
- Memory measurements: GitHub issue tauri#5889 reports that, counting shared memory, a Tauri test app on Windows 10 used 399 MB against 318 MB for Electron. On Ubuntu, default apps measured 118 MB USS for Electron and 125 MB for Tauri. The author concludes Tauri on WebView2 is close to Electron — [tauri#5889](https://github.com/tauri-apps/tauri/issues/5889)
- Blog claims of Tauri idling at 30–80 MB are not backed by reproducible methodology and conflict with the issue above — [tech-insider Tauri vs Electron 2026](https://tech-insider.org/tauri-vs-electron-2026/), [OpenReplay comparison](https://blog.openreplay.com/comparing-electron-tauri-desktop-applications/)

### Inferences
- WebView2 is shared with Edge on Windows, so download size is near zero. Process memory is still a few browser processes (browser, GPU, renderer), plausibly 100–200 MB once a book is open. The webview only needs to exist while the reader is open; destroying it on close returns the memory. That keeps the audiobook-player footprint small.
- On Linux, embedding WebKitGTK into Slint, iced, or egui is the hard part. winit + Wayland child windows do not work with `build_as_child`, so practical options are (1) a separate top-level reader window owned by wry/tao (simplest and robust), or (2) a GTK4 app with `webkit6` widgets. Opening the reader as its own window avoids most integration problems.
- Integration pattern: serve the EPUB bytes to the webview through a custom protocol (wry `with_custom_protocol`), so the auth token never reaches JS. Use IPC messages for `relocated` events (CFI + fraction) that Rust forwards to the ABS PATCH endpoint, plus theme and font-size commands.

### Gaps
- No measured numbers for a WebKitGTK reader window (Foliate) or a WebView2 window displaying a single EPUB. A quick PSS/USS measurement on the target machines is recommended.
- Did not check whether foliate-js tagged releases since 2025.

## 3. EPUB approach (b): native HTML/CSS engines

### Takeaway
No permissively licensed, mature, paginating native EPUB engine exists for Rust. The mature native engines are crengine-ng (GPLv2) and MuPDF (AGPL), both copyleft. litehtml (BSD) and Blitz (MIT/Apache, beta) are usable HTML layout engines, but you would write pagination, CFI, and EPUB handling yourself. Readium's old C++ SDK is unmaintained.

### Cited Findings
- crengine-ng 0.9.12 is "a cross-platform library for text viewers and e-book readers" (GitLab coolreader-ng/crengine-ng), licensed GPLv2 per FreshPorts and GPL-2.0-or-later per Nixpkgs. Build deps include harfbuzz and fribidi — [FreshPorts crengine-ng](https://freshports.org/deskutils/crengine-ng), [nixpkgs commit](https://code.ornl.gov/nix/nixpkgs/-/commit/dec30357b5802dfe3ae18d6d1e8c5156a2bd564e)
- crqt-ng is a Qt GUI reader built on crengine-ng (GPLv2) — [LinuxLinks crqt-ng](https://www.linuxlinks.com/crqt-ng-ebook-reader-crengine-ng/)
- KOReader uses MuPDF, djvulibre, CREngine and libk2pdfopt with a LuaJIT frontend. crengine handles reflowable formats (EPUB, FB2, MOBI) and MuPDF handles PDF/CBZ — [koreader-base mirror](https://github.com/NiLuJe/koreader-base), [kindlepdfviewer](https://github.com/xeonxu/kindlepdfviewer)
- MuPDF opens PDF, XPS, CBZ, EPUB and FB2 — [PyMuPDF docs](https://pymupdftest.readthedocs.io/en/stable/intro.html). It is dual-licensed AGPL or commercial from Artifex. Latest stable listed is 1.27.2 (18 Feb 2026) — [Wikipedia MuPDF](https://en.wikipedia.org/wiki/MuPDF)
- litehtml is under the New BSD license (gumbo parser under Apache 2.0). It draws nothing itself; the host implements a callback container for text, images and fonts. Qt Assistant bundles a copy — [litehtml README mirror](https://github.com/Arkarit/litehtml/blob/master/README.md), [MSYS2 package](https://packages.msys2.org/package/mingw-w64-ucrt-x86_64-litehtml), [Qt attribution](https://qt.developpez.com/doc/6.4/qtassistant-attribution-litehtml)
- Blitz (Dioxus) is in beta with "many bugs and missing features". It uses Stylo for CSS and Taffy for layout and supports flexbox, grid, table, block, inline. `blitz-paint` emits AnyRender commands rendered by `blitz-renderer-vello`, and there is a `wgpu_texture` example. Pagination is not mentioned. Dual MIT/Apache, with `stylo_taffy` MPL-2.0 — [Blitz](https://github.com/DioxusLabs/blitz)
- Readium: EDRLab says the original Readium SDK has not been actively maintained since 2019, and work moved to Readium Mobile, Desktop and Web — [EDRLab](https://www.edrlab.org/readium/readium-sdk-js/), [Readium release 30](https://readium.org/news/_posts/release-30/). Thorium is built on the Readium Desktop toolkit (Electron, BSD-3) — [Thorium docs](https://thorium.edrlab.org/en/docs/900_about/)
- Rust parsing: `rbook` 0.7.0 reads, builds and edits EPUB 2/3 (metadata, manifest, spine, TOC, lazy resource access), Apache-2.0 — [rbook docs.rs](https://docs.rs/crate/rbook/0.7.0), [lib.rs rbook](https://lib.rs/crates/rbook)

### Inferences
- MuPDF is the most attractive single native engine (EPUB + PDF + CBZ, fast, small), but AGPL forces the whole app to be AGPL or requires a paid license. If the app will be open source under GPL-compatible terms (AGPL-3.0, like Readest), this is acceptable. MuPDF's EPUB layout is page-image based with limited CSS and no CFI, so CFI sync must be approximated from chapter/page.
- crengine-ng gives KOReader-quality reflowable rendering with pagination, hyphenation, and its own position "xpointers" (not CFI), but it is GPLv2(+) C++ with a heavy build. It fits a Qt/C++ app better than Rust.
- Blitz + rbook is the most "pure Rust" route but means writing pagination (CSS columns are probably unsupported), selection, and CFI generation yourself, on a beta engine. Too much work for a "simple reader" in 2026.
- litehtml + rbook (C++ or Rust via FFI) is a viable middle path: BSD, small, renders into any canvas. You still do pagination yourself, for example by laying out at a fixed width and slicing by height at block boundaries.

### Gaps
- Did not verify Blitz's latest version or whether it supports CSS multi-column.
- Did not check active Rust litehtml bindings, or bk/epr-style terminal readers (they convert to plain text and are not useful as rendering references).
- crengine-ng's exact license text (v2-only or v2+) was not confirmed from its LICENSE file.

## 4. EPUB approach (c): convert XHTML to the GUI framework's rich text

### Takeaway
Converting chapter XHTML into the toolkit's text model is the lowest-footprint option and is feasible for a "simple" reader of novels. It loses complex CSS, tables, floats and fixed-layout books. Qt's QTextDocument already accepts an HTML 4 subset with basic CSS. In Rust, you would parse XHTML (html5ever/roxmltree) and map it to styled spans for the toolkit.

### Cited Findings
- QTextDocument supports a subset of HTML 4 plus CSS properties including background-color, background-image, color, font-family, font-size, font-style and font-weight, plus Qt-specific properties such as `-qt-block-indent` — [Qt 6 Supported HTML Subset](https://doc.qt.io/qt-6/richtext-html-subset.html), [Qt 6.10 version](https://doc.qt.io/qt-6.10/richtext-html-subset.html)
- rbook provides lazy access to spine items and the TOC, which is what a converter needs as input — [rbook docs.rs](https://docs.rs/crate/rbook/0.7.0)

### Inferences
- Rust GUI rich-text capability (not researched in depth here): egui has `LayoutJob` for styled text runs; iced has `rich_text` spans; Slint's text support is more limited. A converter mapping p/h1-6/em/strong/blockquote/img/a to paragraphs and spans covers most fiction.
- Paginating converted text means measuring laid-out paragraph heights and splitting. Scrolling mode is far simpler and is a reasonable first version.
- CFI generation is easy here, since you keep the source DOM element indices for every block you emit.

### Gaps
- No sources fetched on egui/iced/Slint rich-text APIs or on existing projects that do this conversion.

## 5. PDF rendering

### Takeaway
pdfium via `pdfium-render` (Rust) is the default for a permissive license and good fidelity, with the pdfium shared library shipped beside the app. Qt PDF (LGPLv3, pdfium-based) is the easy path for Qt. MuPDF is excellent but AGPL. Poppler is GPL and awkward on Windows. hayro is pure Rust but experimental and CPU-only.

### Cited Findings
- pdfium-render 0.9.4 (latest at fetch time) binds Pdfium build 7881 through the `pdfium_latest` feature. By default it binds dynamically at runtime with `Pdfium::bind_to_library()` or `bind_to_system_library()`. A `static` feature and `PDFIUM_STATIC_LIB_PATH` allow static linking. It does not build Pdfium itself; prebuilt native binaries come from bblanchon/pdfium-binaries — [pdfium-render](https://github.com/ajrcarey/pdfium-render)
- Qt PDF was relicensed to LGPLv3 (with a commercial option); Qt 6 headers list LGPLv3 / GPLv2+ / commercial. MSYS2 ships qt6-pdf 6.10.1 — [qtpdf git log](https://code.qt.io/cgit/qt-labs/qtpdf.git/log/LICENSE.FDL), [MSYS2 qt6-pdf](https://packages.msys2.org/base/mingw-w64-qt6-pdf)
- MuPDF: AGPL or commercial. Per Artifex's explanation, an application using MuPDF under AGPL must be distributed under AGPL — [Robin Watts, MuPDF Explored, Licensing](https://ghostscript.com/~robin/mupdf_explored/mupdf_exploredse159.html), [Wikipedia MuPDF](https://en.wikipedia.org/wiki/MuPDF)
- hayro (Laurenz Stampfl): experimental pure-Rust, CPU-only PDF renderer with a regression suite of over 1000 PDFs, `forbid(unsafe_code)`. Known gaps: no encrypted PDFs, no blending/isolation or knockout groups, performance not yet a focus. Versions 0.5–0.7.1 were seen on docs.rs — [hayro docs.rs](https://docs.rs/hayro), [hayro 0.7.1](https://docs.rs/crate/hayro/0.7.1)
- Readest and Foliate render PDF through PDF.js inside the webview (foliate-js marks PDF experimental) — [foliate-js](https://github.com/johnfactotum/foliate-js), [Readest](https://github.com/readest/readest)

### Inferences
- pdfium is BSD-3/Apache-style (Google), so it is compatible with any app license. Expect a single pdfium.dll/libpdfium.so of several MB (not measured here). Load it lazily only when a PDF is opened.
- If the EPUB path uses a webview anyway, PDF.js inside the same webview avoids a second native dependency, at the cost of slower rendering of large scanned PDFs.
- hayro is worth watching for a zero-C-dependency build, but it lacks encrypted-PDF support. Many PDFs are encrypted with only owner restrictions, so it is risky as the only renderer in 2026.
- Poppler (GPL-2/3) is easy on Linux via distro packages but must be bundled with many dependencies on Windows. It offers nothing over pdfium for this use.

### Gaps
- Exact pdfium binary sizes from bblanchon/pdfium-binaries were not fetched.
- pdfium-render's own license was not stated on the fetched page (believed MIT/Apache; verify LICENSE.md).
- hayro's latest version as of Oct 2026 and whether encryption support landed were not confirmed.

## 6. CBZ/CBR comics

### Takeaway
CBZ is trivial: a zip of images, read with any zip crate plus an image decoder. CBR needs RAR decoding. The official unrar source is free to use for extraction but is not an open-source license. libarchive/unrar-free have limited RAR support. Supporting CBZ only, or CBR through optional unrar, is reasonable.

### Cited Findings
- The unrar license allows building software that unpacks RAR archives without a fee, but not software that creates them, and it is not a free software license. The RAR license agreements forbid reverse engineering — [Wikipedia RAR](https://en.wikipedia.org/wiki/RAR_(file_format))
- rarfile's backend notes rate bsdtar (libarchive) as "not recommended: limited RAR format support", and say unrar-free is based on libarchive with similar support plus multi-volume — [rarfile docs](https://rarfile.readthedocs.io/), [rarfile PyPI](https://pypi.org/project/rarfile/)
- ABS's web UI includes a comic reader for CBZ, and MuPDF/KOReader and foliate-js open CBZ natively — [foliate-js](https://github.com/johnfactotum/foliate-js), [koreader mirror](https://github.com/NiLuJe/koreader-base)

### Inferences
- In Rust: `zip` + `image` crates, natural-sort the entries, decode one or two pages ahead and keep only a small LRU of decoded bitmaps to keep memory low. Progress for comics is a page index.
- The `unrar` Rust crate wraps the RARLAB source, so it carries the same non-free license. That is acceptable for proprietary or permissive apps, but distros like Debian treat it as non-free, which matters for Flatpak/distro packaging.

### Gaps
- Did not verify libarchive's RAR5 read support status in 2026, or what the ABS ComicReader does for CBR (it may extract server-side or client-side).

## 7. MOBI/AZW3

### Takeaway
Low priority. Most ABS libraries are EPUB, and DRM-protected Kindle files cannot be read anyway. If wanted, foliate-js already reads MOBI/KF8 in the webview approach, and libmobi (LGPL-3.0+) can parse it natively.

### Cited Findings
- foliate-js supports MOBI and KF8 (AZW3), but KF8 decompression is slow — [foliate-js](https://github.com/johnfactotum/foliate-js)
- libmobi is LGPL-3.0-or-later and reads MOBI including KF8 (azw, azw3), reconstructs HTML references, and handles encrypted documents — [Homebrew libmobi](https://formulae.brew.sh/formula/libmobi), [Guix libmobi](https://packages.guix.gnu.org/packages/libmobi/)
- Upstream EPUB export in libmobi was not confirmed. A fork (gabb99/libmobi) lists EPUB export — [gabb99/libmobi](https://github.com/gabb99/libmobi)
- crengine (KOReader, CoolReader) also opens MOBI — [kindlepdfviewer](https://github.com/xeonxu/kindlepdfviewer)

### Inferences
- With a native engine, the cheapest path is to skip MOBI or offer "open in system viewer". ABS has no server-side conversion, so mention that users can convert to EPUB with Calibre.
- CFI sync is not meaningful for MOBI in the ABS web reader, which only reads EPUB through epub.js, so cross-client progress for MOBI would be percentage only.

### Gaps
- Did not check whether the ABS web client opens MOBI/AZW3 at all, or offers download only.

## 8. Existing open-source readers and their engines

### Takeaway
Nearly every modern cross-platform EPUB reader uses a web engine (WebKitGTK, Chromium via Electron/QtWebEngine, or system webview via Tauri). The native exceptions use copyleft engines (crengine, MuPDF). The webview path is the proven one; the native path is proven only under GPL/AGPL.

### Cited Findings
- Foliate: GTK4 (4.12+), libadwaita 1.8+, webkitgtk-6.0, gjs 1.82+, foliate-js; GPL-3.0-or-later; Linux only (Flatpak, Snap, distro packages); formats EPUB, MOBI, AZW3, FB2, CBZ, OPDS — [Foliate](https://github.com/johnfactotum/foliate)
- Readest: rewrite of Foliate with Next.js 16 + Tauri v2 + foliate-js + PDF.js; AGPL-3.0-or-later; Windows, Linux, macOS, Android, iOS, web; formats EPUB, PDF, MOBI, AZW3, FB2, CBZ, TXT, HTML, MD; syncs progress/notes/bookmarks across its platforms and with KOReader — [Readest](https://github.com/readest/readest)
- KOReader: crengine for reflowable, MuPDF for PDF/CBZ, djvulibre, k2pdfopt, LuaJIT UI — [koreader-base mirror](https://github.com/NiLuJe/koreader-base)
- Thorium: Electron + Readium Desktop toolkit, BSD-3 — [Thorium docs](https://thorium.edrlab.org/en/docs/900_about/), [gittrend Thorium](https://gittrend.io/repo/edrlab/thorium-reader)
- crqt-ng: Qt GUI on crengine-ng, GPLv2 — [LinuxLinks crqt-ng](https://www.linuxlinks.com/crqt-ng-ebook-reader-crengine-ng/)

### Inferences
- Readest is the closest model for a Tauri-based ABS reader. Its source shows how to drive foliate-js from a host app (progress events, themes, font settings), but its AGPL license means code cannot be copied into a non-AGPL app. foliate-js itself is MIT.
- From background knowledge, not verified this session: Calibre's viewer uses Qt WebEngine; Sioyek and zathura (zathura-pdf-mupdf plugin) use MuPDF; Okular uses Poppler for PDF and its own generators for EPUB; Bookworm (elementary) uses WebKitGTK. These were not fetched.

### Gaps
- No memory measurements for Foliate, Readest, KOReader desktop, or Calibre viewer were found.
- Rust-native readers (bk, epr, etc.) were not reviewed; they are terminal text readers.

## 9. Pagination, themes, TOC, and recommendations per GUI stack

### Takeaway
Pagination via CSS columns, themes via injected CSS, font sizing, and TOC navigation are already in foliate-js and epub.js. A native engine needs all of these written by hand except with crengine/MuPDF. For a "simple" reader: scroll mode, light/dark/sepia themes, font-size steps, a TOC sidebar from the EPUB nav/NCX, and CFI + percentage progress.

### Cited Findings
- foliate-js paginator: CSS multi-column, paginated and scrolled modes, location anchoring across resizes — [foliate-js](https://github.com/johnfactotum/foliate-js)
- ABS web reader computes chapter percentages from generated locations and saves the CFI of the first visible position — [ABS EpubReader.vue](https://github.com/advplyr/audiobookshelf/blob/master/client/components/readers/EpubReader.vue)
- rbook exposes the TOC for native implementations — [rbook docs.rs](https://docs.rs/crate/rbook/0.7.0)

### Inferences (recommendations)
- **Tauri / webview app:** the obvious fit. Use foliate-js (git submodule, MIT) for EPUB/MOBI/AZW3/FB2/CBZ and PDF.js or pdfium for PDF. Forward the CFI from `relocate` events to the ABS PATCH. Lowest effort and best CFI compatibility.
- **Rust Slint / iced / egui (winit):** open a separate reader window with wry/tao (WebView2 on Windows; WebKitGTK via `build_gtk` on Linux, which needs GTK on the Linux side) hosting foliate-js. The main app stays small, and the webview memory is paid only while reading. Use pdfium-render for PDF and zip+image for CBZ natively in the main UI. Fallback if no webview is wanted: rbook + an XHTML-to-rich-text converter in scroll mode, with spine-level CFIs.
- **GTK4 (gtk-rs):** embed `webkit6` WebView directly with foliate-js, the same as Foliate. This is the cleanest on Linux. On Windows, WebKitGTK is not practical, so this stack is weak for Windows unless EPUB uses a different engine there.
- **Qt/C++ (or Qt via Rust bindings):** two good options. (1) QtWebEngine + foliate-js: heavy, since it bundles Chromium (~100+ MB on disk). (2) crengine-ng for EPUB/FB2/MOBI + Qt PDF for PDF, a native and low-memory setup that is GPLv2, so the app must be GPL. A third, lighter path is QTextBrowser/QTextDocument with the HTML subset for basic novels. Qt PDF (LGPLv3) covers PDF.
- **.NET Avalonia:** WebView2 on Windows and WebKitGTK on Linux through a community webview control (not researched) hosting foliate-js, plus PDFium via a .NET wrapper (for example PDFiumCore, not verified here). The native fallback is the same XHTML-to-rich-text conversion.
- **Licensing summary:** an app intended as AGPL/GPL open source can use MuPDF (one engine for EPUB+PDF+CBZ) or crengine. Otherwise use foliate-js (MIT) + pdfium (BSD) + zip/image, avoid AGPL MuPDF, and treat unrar as optional and non-free.
- **CFI priority:** high for EPUB if users also read in the ABS web or mobile client; this pushes towards foliate-js/epub.js. If users only read on desktop, percentage plus spine-level CFI is enough.

### Gaps
- Avalonia webview controls and .NET PDFium wrappers were not checked for maintenance status.
- No direct memory comparison between a native engine (MuPDF/crengine) and a webview reader was found; native engines are expected to be in the tens of MB, but no source was retrieved for that.
