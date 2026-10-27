# ABS Desktop

A desktop client for [Audiobookshelf](https://www.audiobookshelf.org/) on Windows and Linux, written in Rust with Slint and libmpv.

It works with Audiobookshelf 2.26.0 or later.

## What it does

- Several servers and accounts at once. A book that is on more than one of them shows up once, and you can play it from whichever copy you like.
- Streaming, plus downloads for offline listening. Listening time recorded offline is sent to the server once it is reachable again. If another device moved the position in the meantime, the app asks which one to keep.
- Progress stays in sync through the server's realtime events. When you pause, the app picks up progress made on other devices.
- An ebook reader for EPUB, PDF and comic archives (CBZ), based on [foliate-js](https://github.com/johnfactotum/foliate-js). It runs in a separate window and remembers the reading position.
- Chapters, bookmarks, playback speed, a sleep timer that can stop at the end of a chapter, a mini player and a full screen player.
- Media keys and the system media overlay (SMTC on Windows, MPRIS on Linux).
- Themes, including Catppuccin, Material, WinUI 3 with Mica, and Glass. Fonts, corner radius, density and borders can be changed, and themes can be imported and exported.

## Logging in

Enter the server address and either your username and password or an API key.

If the server only allows single sign-on (OpenID Connect), use an API key. The app can't log in through SSO yet. In the Audiobookshelf web interface, an admin opens Settings, API Keys, adds a key and picks the user it acts for.

## Install

Every [release](https://github.com/LaurinSorgend/ABSDesktopApp/releases) has these files:

| Platform | File | Updates |
|---|---|---|
| Windows | `AbsDesktop-win-Setup.exe`, a per-user installer, no admin rights needed | In the app, Settings, Updates |
| Windows | `AbsDesktop-win-Portable.zip`, unpack anywhere | In the app, Settings, Updates |
| Windows | `abs-desktop-<version>-windows-x64.zip`, the package for winget and Scoop | Through the package manager |
| Linux | `abs-desktop-<version>-x86_64.AppImage` | Download the new version |
| Linux | `abs-desktop_<version>_amd64.deb` (Debian, Ubuntu) | Download the new version |
| Linux | `PKGBUILD` for Arch Linux | Through your AUR helper, once the package is on the AUR |

Scoop can install straight from the manifest attached to a release:

```
scoop install https://github.com/LaurinSorgend/ABSDesktopApp/releases/download/v<version>/abs-desktop.json
```

winget and the AUR are planned. Flathub will follow.

Only the Windows installer and its portable zip update themselves. Every other channel leaves updates to the package manager, so two updaters never fight over one installation.

On Linux the app needs libmpv, WebKitGTK 4.1 (for the reader) and `libxkbcommon-x11`, which the window system loads at runtime. The `.deb` and the PKGBUILD pull these in. For the AppImage on a minimal system, install `libxkbcommon-x11-0` (Debian, Ubuntu) or `libxkbcommon-x11` (Arch, Fedora).

## Where data is kept

Settings, the library cache and downloads are in your user data folder (`%APPDATA%\ABS Desktop` on Windows, `~/.local/share/abs-desktop` on Linux). Covers are cached in `%LOCALAPPDATA%\ABS Desktop` or `~/.cache/abs-desktop`. Logins are stored in Windows Credential Manager or in the Secret Service keyring (GNOME Keyring, KWallet). Without a keyring they go into a private file in the data folder, and the app says so.

## Building

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

GPL-3.0-or-later, see [LICENSE](LICENSE). The release packages list the licenses of everything bundled with the app in `THIRD-PARTY-LICENSES.html` and the `licenses` folder. The app icon is the Audiobookshelf logo, which is GPL-3.0 as well.

ABS Desktop is not made by the Audiobookshelf project.
