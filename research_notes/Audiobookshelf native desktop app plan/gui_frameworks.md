# Native cross-platform GUI frameworks for an Audiobookshelf desktop client (Windows + Linux)

Research date: 2026-10-24. About 19 tool calls. Coverage is uneven: Rust stacks, Tauri, Qt, Avalonia and Compose have sourced data. Fyne, Gio, Wails, Flutter desktop, wxWidgets, Dioxus Blitz and gtk4 packaging on Windows could not be verified this session (see Gaps). Anything under "Inferences" that is not tied to a cited finding comes from background knowledge and should be re-checked before it goes into the final report.

## Idle/typical memory, distributable size, startup time

### Takeaway
I found no single current benchmark that measures all candidates the same way. The sourced numbers are old or loosely measured. Toolkits that draw natively (Qt Widgets, GTK, Slint, egui, iced) sit at roughly 15-100 MB idle. JVM and Flutter stacks sit around 100 MB. Tauri on WebView2/WebKitGTK is close to Electron once shared webview processes are counted, and the "30-50 MB" figures for Tauri are unsourced blog numbers.

### Cited Findings
- 2019 measurement of private memory with ksysguard on Ubuntu 19.04 (author calls it "not scientific"): Qt 17 MB, Gtk+3 5 MB, wxGtk3 6 MB, FLTK 1.7 MB, Dear ImGui (SDL) 15.3 MB, Java Swing 59.3 MB, JavaFX 80 MB, Electron 74.6 MB, Flutter Desktop 98 MB (early, debug builds only). — [szibele.com, Memory Footprint of GUI Toolkits (16 Aug 2019)](https://szibele.com/memory-footprint-of-gui-toolkits/)
- Ubuntu 20.04/X11, Tauri 1.0 vs iced 0.7 vs egui 0.20, release builds: startup ≈380 ms (window after ≈125 ms) for Tauri, ≈230 ms (window after ≈33 ms) for iced, ≈280 ms for egui. Binary size 5 MB for Tauri, 17 MB for iced, 18 MB for egui. Input lag 2-3 / 3 / 2 frames. Memory was not measured. The author warns against generalizing beyond that Linux setup. — [Lukas Kalbertodt, Tauri vs Iced vs egui (3 Feb 2023)](http://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html)
- Tauri issue #5889 (Dec 2022-May 2024) with Postman web app loaded: Tauri 421 MB (macOS), 581 MB (Ubuntu), 399 MB (Windows 10) vs Electron 337 / 240 / 318 MB. vscode.dev: Tauri 429 / 572 / 370 MB vs Electron 332 / 222 / 312 MB. Ubuntu default apps: Electron 118 MB USS / 207 MB PSS, Tauri 125 MB USS / 185 MB PSS. Tauri on WebView2 used memory similar to Electron, and WebKit webviews added 90+ MB for web apps. The issue was closed without a definitive conclusion. — [tauri-apps/tauri#5889](https://github.com/tauri-apps/tauri/issues/5889)
- Unsourced blog claims put Tauri idle at 30-50 MB vs Electron at 150-300 MB, and one "July 2026 testing" claim reports 42 MB vs 168 MB with no method given. Both conflict with #5889. — [tech-insider.org, Tauri vs Electron 2026](https://tech-insider.org/tauri-vs-electron-2026/); [gpui-starter, Rust GUI frameworks 2026](https://gpui-starter.freeoxide.com/blog/rust-gui-frameworks-comparison-2026/)
- An unverified 2025 review gives typical memory of about 30-60 MB for egui and 40-100 MB for iced, without methodology. — [AN4T, Rust GUI Libraries Compared (2025)](https://an4t.com/rust-gui-libraries-compared/)
- Compose Multiplatform desktop, anecdotal Kotlin Slack reports from 2021-2023: an empty window uses about 100 MB on macOS and less on Windows. One Windows MSI user saw about 100 MB RAM at startup and about 103 MB install size. jpackaged apps run about 100-123 MB because they bundle a JRE. GraalVM native image experiments gave a zipped binary of about 25 MB. — [kotlinlang Slack archive](https://slack-chats.kotlinlang.org/t/519199/why-does-an-empty-desktop-compose-app-just-showing-an-empty-); [kotlinlang Slack (GraalVM)](https://slack-chats.kotlinlang.org/t/33108914/i-managed-to-get-compose-working-with-graalvm-on-macos-which)
- PySide/PyQt packaged sizes for a minimal PyQt6 app: about 30-80 MB for a Windows PyInstaller exe and 40-90 MB for a Linux AppImage, growing with Qt modules. QtWebEngine is "one of the heaviest Qt modules", and pyside6-deploy drops unused heavy modules to shrink output. — [pythonguis.com FAQ](https://www.pythonguis.com/faq/packaged-installer-file-sizes/); [Qt for Python pyside6-deploy docs](https://doc.qt.io/qtforpython-6/deployment/deployment-pyside6-deploy.html)
- Unverified 2025 figures: Qt executables about 8-25 MB, Flutter desktop bundles about 25-40 MB, with Flutter using more memory. — [markaicode, Flutter Desktop vs Qt 2025](https://markaicode.com/flutter-desktop-vs-qt-performance-comparison/)
- A 2026 roundup states that Qt uses the least idle memory of the seven frameworks it covers (no numbers in snippet). — [TeamDev, Top 7 desktop app frameworks in 2026](https://teamdev.com/mobrowser/blog/top-7-desktop-app-frameworks-in-2026/)
- The "ultimate-ui-framework-benchmark" repo (build dated 13 Oct 2026) has only one filled row (~18 ms startup, ~32 MB, 4.98 MB, likely egui on Android). The rest says "To be filled in after testing", so it is not usable for desktop comparison. — [shubham-sharma-1994/ultimate-ui-framework-benchmark](https://github.com/shubham-sharma-1994/ultimate-ui-framework-benchmark)

### Inferences
- For an image-heavy library grid, decoded cover bitmaps (for example 200 covers at 300x300 RGBA ≈ 72 MB uncompressed) will dominate memory more than the toolkit baseline. Thumbnail downscaling and virtualization matter more than the toolkit's hello-world RSS.
- Tauri's memory advantage over Electron is mostly disk/download size (≈5 MB binary vs ~100+ MB). Runtime RAM with a real web UI is in the same range as Electron per #5889. This conflicts with the "small/efficient" requirement on Windows, where WebView2 is Chromium.
- Background knowledge, not verified: Slint publishes very small footprints for embedded targets, and its desktop apps use winit + femtovg/skia, so expect egui/iced-class RAM. Avalonia self-contained builds are usually 30-70 MB, and NativeAOT trims this. Fyne/Gio Go binaries are usually 10-30 MB.

### Gaps
- No reproducible 2024-2026 benchmark measuring idle RSS/PSS for Slint, iced 0.14, gtk4-rs, Avalonia 12, Fyne, Gio, Wails, Flutter 3.x and Qt 6 on Windows 11 and a Wayland Linux desktop. The ironoxidizer GUI toolkit benchmark page returned 404. A custom measurement is recommended (same cover-grid prototype, measure Private Working Set on Windows and PSS on Linux).
- No sourced numbers for Fyne, Gio, Wails, wxWidgets 3.2, Dioxus/Blitz or Avalonia 12.

## Maturity on Windows and Linux (Wayland/X11, HiDPI, accessibility, IME, native look, tray, dialogs)

### Takeaway
On Windows, a hands-on 2025 survey found only Slint, Dioxus (WebView2) and Tauri passed both screen-reader and IME tests among Rust toolkits. GTK4/Relm4 worked for IME but failed Narrator on Windows. iced failed both at that time, though iced 0.14 (Dec 2025) has since added IME. Qt remains the most complete on both OSes. Avalonia got a native Wayland backend in 12.1, still opt-in and experimental.

### Cited Findings
- April 2025 Windows survey (Narrator + Japanese IME): Slint "works fully" with Narrator, and IME works after commit with missing glyphs in provisional state. GTK 4 and Relm4: Narrator "can't see text or window buttons", IME works, non-native look. iced: Narrator can't see anything, IME "won't activate". egui: Narrator sees text but the text field is janky, IME composer works but Tab is eaten, and the default font lacks kanji. Tauri and Dioxus work via WebView2. CXX-Qt and qmetaobject failed to build. The author rated Slint cleanest overall. — [boringcactus, 2025 Survey of Rust GUI Libraries (13 Apr 2025)](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)
- iced 0.14 (released 7 Dec 2025) adds input method support, hot reloading, reactive rendering, time-travel debugging, headless testing, grid and table widgets, and image scaling. Release notes do not mention accessibility. — [iced 0.14.0 release](https://github.com/iced-rs/iced/releases/tag/0.14.0); [Phoronix](https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary)
- COSMIC was rebuilt as a standalone desktop on iced. COSMIC Epoch 1.1.0 (June 2026) added COSMIC Monitor, built with the same toolkit. — [Wikipedia: COSMIC](https://en.wikipedia.org/wiki/COSMIC_desktop); [OMG! Ubuntu (June 2026)](https://www.omgubuntu.co.uk/2026/06/cosmic-desktop-update-system-monitor)
- Avalonia 12.1: the native Wayland backend graduated from private preview but is experimental. It is not chosen by UsePlatformDetect(), so you opt in via the Avalonia.Wayland package (12.1.1 on NuGet) and UseWayland()/UseWaylandWithFallback. It renders through EGL. Some KDE niceties (global menu, window icons, blur) are still pending. X11 got full XDND drag and drop. — [Avalonia blog: What's new in 12.1](https://avaloniaui.net/blog/release-12-1); [NuGet Avalonia.Wayland](https://www.nuget.org/packages/avalonia.wayland/); [Avalonia v12 supported platforms](https://v12.docs.avaloniaui.net/docs/supported-platforms)

### Inferences
- Background knowledge, not verified: Qt 6 has native Wayland (including fractional-scale-v1 since 6.5/6.6), UI Automation on Windows, AT-SPI on Linux, QSystemTrayIcon and native file dialogs via portals. It is the lowest-risk choice on platform integration.
- Background knowledge: Slint and iced use winit, which supports Wayland and X11 with fractional scaling. System tray is not built into either. The usual route is the `tray-icon` crate (from the Tauri org) plus `rfd` for native/portal file dialogs.
- GTK4 on Windows works, but it draws Adwaita-style client-side decorations, and its Windows accessibility gap (per boringcactus) makes it a poor fit if Windows is a first-class target. libadwaita on Windows looks like GNOME, not Windows.
- iced's accessibility is still missing per the 0.14 notes. That is a real gap for a media client but may be acceptable for a personal project.

### Gaps
- Could not verify current Slint 1.18 (16 Sep 2026) changelog details on accessibility/IME fixes since April 2025. The GitHub release page only links to the changelog. — [Slint releases](https://github.com/slint-ui/slint/releases)
- No verified data on Fyne/Gio/Flutter Wayland fractional scaling or accessibility on Windows.

## Image-heavy grids, async/networking, theming

### Takeaway
iced 0.14 has a new grid widget and image scaling. Slint has ListView virtualization and async integration via its event loop. Qt has QListView/QML GridView with model-based virtualization. Avalonia has virtualizing panels. All are workable. Rust stacks pair naturally with tokio/reqwest.

### Cited Findings
- iced 0.14 adds `grid` widget/`grid!` macro, `table` widget, image `scale`, "floating images" and Markdown rendering with image support. — [iced 0.14.0 release](https://github.com/iced-rs/iced/releases/tag/0.14.0)
- Slint: clean DSL with `<=>` two-way bindings. Error messages were weak in 2025. — [boringcactus survey](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)

### Inferences
- Background knowledge: Slint's ListView virtualizes rows, but there is no built-in virtualized *wrapping grid*. The common pattern is a ListView of rows each holding N covers, computed from the window width. Images load via `slint::Image::from_rgba8` after decoding on a worker thread and are pushed back with `invoke_from_event_loop`/`upgrade_in_event_loop`.
- Background knowledge: iced's Elm architecture uses `Task`/`Subscription` for async (tokio or smol runtime). `image::Handle::from_bytes` handles covers. Whether the new grid widget virtualizes has not been verified.
- Background knowledge: Qt QML GridView plus QQuickImageProvider/async Image loading is the most mature path for a virtualized cover grid.

### Gaps
- No source confirming whether iced 0.14's grid is lazy/virtualized.

## Rich text / HTML for an ebook reader

### Takeaway
Only Qt (QTextBrowser for basic HTML; QtWebEngine for full EPUB CSS, though heavy) and webview-based stacks (Tauri, Wails, Dioxus desktop) render HTML/EPUB content out of the box. Slint and iced have no HTML renderer. iced has Markdown and rich_text widgets. For those, options are a separate webview window (`wry`), converting EPUB XHTML to the toolkit's rich-text spans, or embedding Blitz (Dioxus' HTML/CSS engine), which is still early.

### Cited Findings
- iced 0.14's Markdown widget got incremental parsing, customizable rendering with image support, quotes and task lists. `rich_text` exists, with link click handling. There is no webview. — [iced 0.14.0 release](https://github.com/iced-rs/iced/releases/tag/0.14.0)
- QtWebEngine is one of the heaviest Qt modules for packaging. — [Qt for Python pyside6-deploy docs](https://doc.qt.io/qtforpython-6/deployment/deployment-pyside6-deploy.html)
- Dioxus desktop renders via WebView2 on Windows ("Diet Electron" in the survey's words). — [boringcactus survey](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)

### Inferences
- A pragmatic hybrid for Slint/iced: open the ebook reader in a separate `wry` webview window (the WebView2/WebKitGTK cost is paid only while reading), or use the server-side ebook endpoints and render simplified XHTML as styled text. Audiobookshelf ebooks are mostly EPUB/PDF. PDF needs a separate renderer (pdfium/mupdf bindings) in any stack.
- Qt Widgets QTextBrowser supports a subset of HTML4/CSS2. It is fine for reflowable EPUB chapters without complex CSS and avoids the WebEngine size penalty.

### Gaps
- Blitz (Dioxus native renderer) maturity and release status as of Oct 2026 not verified.

## Media integration (audio backend, MPRIS/SMTC)

### Takeaway
Media integration is mostly independent of the GUI toolkit. In Rust, `souvlaki` gives one API for MPRIS (Linux, D-Bus or pure-Rust zbus backend) and Windows SMTC (needs a window handle via raw-window-handle, which winit-based Slint/iced expose). Qt has QtMultimedia but no built-in MPRIS/SMTC.

### Cited Findings
- souvlaki 0.8.0: cross-platform media controls on Linux (MPRIS), Windows and macOS. Windows needs an HWND obtainable via raw_window_handle/winit. Linux offers dbus-crossroads (default) or a pure-Rust zbus backend. — [docs.rs souvlaki 0.8.0](https://docs.rs/crate/souvlaki/0.8.0); [GitHub Sinono3/souvlaki](https://github.com/Sinono3/souvlaki)

### Inferences
- Background knowledge: Rust audio backend options are rodio/symphonia (pure Rust decoding; HLS streaming from Audiobookshelf needs extra work) or libmpv/GStreamer bindings. The mpv route handles HLS/progressive streams and chapter seeking, at the cost of shipping libmpv on Windows (~30-40 MB).
- Background knowledge: in C++/Qt, MPRIS is done via QtDBus and SMTC via C++/WinRT. In .NET, SMTC is reachable through Windows SDK projections and MPRIS via Tmds.DBus.

### Gaps
- No verified info on souvlaki maintenance cadence in 2026.

## Maintainability, stability, licensing, tooling

### Takeaway
Slint is 1.x with a regular roughly two-month minor cadence (1.14 in Oct 2025 through 1.18.1 in Sep 2026) and a royalty-free license for desktop apps that requires attribution. iced is still pre-1.0 (0.14, Dec 2025) with breaking releases, but COSMIC gives it a large real-world user. Qt is the most stable but has LGPL obligations (dynamic linking, relinkability, source offer).

### Cited Findings
- Slint releases: 1.14.0 (21 Oct 2025), 1.15.0 (4 Feb 2026), 1.16.0 (16 Apr 2026), 1.17.0 (24 Jun 2026), 1.18.0 (16 Sep 2026), 1.18.1 (21 Sep 2026). Binaries ship for Windows and Linux, plus slint-lsp and slint-viewer tooling. — [Slint GitHub releases](https://github.com/slint-ui/slint/releases)
- Slint license: GPL-3.0-only OR Slint Royalty-free 2.0 OR Slint Software (commercial) 3.0. Royalty-free 2.0 covers desktop, mobile and web apps but not embedded. It requires showing the AboutSlint widget in an About dialog, or an attribution badge on the download page, and forbids redistributing Slint's APIs standalone. — [Slint terms](https://slint.dev/terms-and-conditions); [Royalty-free license PDF](https://slint.dev/agreements/slint-royalty-free-license.pdf); [Espressif registry license string for 1.16.1](https://components.espressif.com/components/slint/slint/versions/1.16.1/license)
- iced 0.14 added hot reloading and time-travel debugging. Still pre-1.0. — [iced 0.14.0 release](https://github.com/iced-rs/iced/releases/tag/0.14.0)
- Qt LGPL per The Qt Company: dynamic linking keeps app code proprietary, while static linking may not. Users must be able to relink/replace Qt (LGPLv3 includes running the modified build on the target). You must ship Qt source or a written offer. Some modules (Qt Charts, Data Visualization, Virtual Keyboard) are not LGPL. — [Qt LGPL obligations](https://www.qt.io/development/open-source-lgpl-obligations); [Qt FAQ 3.7](https://www.qt.io/faq/3.7.-what-are-my-obligations-when-using-qt-under-the-lgpl)
- Relm4 has "hard-to-debug macro errors". Tauri IPC is stringly typed. egui system font loading is awkward. — [boringcactus survey](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)

### Inferences
- If the app is open source (GPL-compatible), Slint's GPLv3 option avoids the attribution clause entirely. Qt LGPL is easy for an open-source app with dynamic linking (the default for Qt installers, PySide6 wheels and distro packages).
- Slint has a live preview (VS Code extension / slint-viewer) and a separate `.slint` DSL. iced has no DSL (pure Rust), which some find easier to refactor and others find verbose.

### Gaps
- Breaking-change frequency for Avalonia 11 to 12, Fyne 2.x and Gio not researched.

## Packaging (MSI/MSIX/winget; AppImage/Flatpak/deb)

### Takeaway
Rust single-binary stacks (Slint, iced, egui) are the simplest to package: cargo-packager/cargo-wix for MSI, cargo-deb, Flatpak via flatpak-cargo-generator. Qt/PySide need windeployqt/pyside6-deploy and grow fast with WebEngine. JVM apps use jpackage and bundle about 100 MB of runtime.

### Cited Findings
- pyside6-deploy (Nuitka-based) strips unused heavy Qt modules to cut size. — [Qt for Python docs](https://doc.qt.io/qtforpython-6/deployment/deployment-pyside6-deploy.html)
- Compose Desktop native distributions via jpackage are about 100-123 MB due to the bundled JRE. — [kotlinlang Slack](https://slack-chats.kotlinlang.org/t/519199/why-does-an-empty-desktop-compose-app-just-showing-an-empty-)
- Tauri produces small binaries (≈5 MB in 2023 test) because it uses the OS webview. — [Lukas Kalbertodt (2023)](http://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html)

### Inferences
- Background knowledge: Tauri 2 has the most turnkey bundler (MSI via WiX, NSIS, deb, rpm, AppImage). Slint/iced projects often reuse `cargo-packager` (same team) for the same outputs. Flathub hosts many GTK and iced (COSMIC) apps, and Flatpak is easiest for GTK/Qt via existing runtimes (org.gnome.Platform, org.kde.Platform).

### Gaps
- No verified per-stack winget/MSIX workflow docs fetched.

## Real-world apps of similar shape

### Takeaway
The verified anchor is COSMIC (iced). Media/podcast clients exist across GTK, Qt, druid and iced, but specific app-to-toolkit claims below are background knowledge and were not fetched this session.

### Cited Findings
- COSMIC desktop apps, including COSMIC Monitor (June 2026), are built on iced. — [Wikipedia COSMIC](https://en.wikipedia.org/wiki/COSMIC_desktop); [OMG! Ubuntu](https://www.omgubuntu.co.uk/2026/06/cosmic-desktop-update-system-monitor)

### Inferences
- Background knowledge (re-check before citing): Amberol, Shortwave and Podcasts (GNOME) use GTK4/libadwaita via gtk-rs. spotify-qt and Strawberry are Qt. psst was a druid (now unmaintained) Spotify client. Spotube is Flutter. Cosmic Player is iced + GStreamer. Several Audiobookshelf third-party clients exist, for example the Flutter-based "Audiobookshelf app" forks.

### Gaps
- Did not verify a list of Slint showcase media apps or existing native Audiobookshelf desktop clients.

## Overall assessment (synthesis for the report writer)

### Takeaway
For a small, maintainable Windows+Linux Audiobookshelf client, the evidence favors (1) Slint (Rust) for the best Windows accessibility/IME results among Rust toolkits, 1.x stability, live preview and a small footprint, with souvlaki for MPRIS/SMTC and a wry webview window or rich-text conversion for the ebook view. (2) Qt 6 (C++ or PySide6) is the most mature option and the only one with a built-in HTML reader widget (QTextBrowser), at the cost of LGPL compliance and larger packages. (3) iced 0.14 is attractive if Linux/COSMIC integration matters more than Windows accessibility. Tauri does not meet the "memory-efficient" goal on Windows according to the #5889 measurements.

### Cited Findings
- Slint cleanest on Windows accessibility/IME in 2025 testing. iced and GTK4 failed Narrator. — [boringcactus survey](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)
- Tauri ≈ Electron RAM on WebView2. — [tauri#5889](https://github.com/tauri-apps/tauri/issues/5889)
- Slint 1.x regular releases. Royalty-free desktop license with attribution, or GPLv3. — [Slint releases](https://github.com/slint-ui/slint/releases); [Slint terms](https://slint.dev/terms-and-conditions)
- iced 0.14 IME + hot reload + grid; no accessibility listed. — [iced 0.14.0](https://github.com/iced-rs/iced/releases/tag/0.14.0)

### Inferences
- Avalonia 12 (C#/XAML) is a credible middle ground with good Windows maturity and improving Wayland, but the .NET runtime adds size. Compose Desktop and Flutter carry about 100 MB-class baselines and fit the "small" goal less well.

### Gaps
- A hands-on prototype benchmark (cover grid of ~500 items plus a playing audio stream) on Windows 11 and Fedora/Ubuntu Wayland would resolve most numeric gaps.
