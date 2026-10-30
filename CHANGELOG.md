# Changelog

Kept by hand. Each release lists what changed for people using the app.

## 0.2.1 (2026-10-30)

- A damaged download can be repaired. The missing files are fetched again at start instead of the download ending as failed, and a file that was fully fetched but not yet renamed no longer fails it.
- Playing audio no longer erases the saved reading position of the same book or marks a finished book as unfinished.
- Reading positions saved while the server answers with an error or the login has expired are kept and sent later. Only a position the server refuses is dropped.
- A downloaded book plays from its files when the server cannot start a session.
- Starting another book no longer lets the previous book's position reach the new one.
- Fewer unexpected logins: a token refresh that gets interrupted keeps the new token, and reconnecting no longer breaks a running playback or download.
- A server that stops answering no longer blocks the player or a download. Requests give up after 60 seconds.
- Progress made on other devices is no longer ignored when this computer's clock runs fast. Choosing the other device's position in a conflict now also updates this computer.
- The play button is centered in the full screen player.

## 0.2.0 (2026-10-27)

- Mini player in a small window that can stay on top, and a full screen player with the blurred cover behind it. F11 toggles full screen.
- A paused book picks up progress made on other devices. Settings sets how often it looks (default every 30 seconds).
- Themes can move the navigation and the player bar and choose how single widgets look.
- Log in with an API key as well as with a password, for servers that only allow single sign-on.
- Windows installer (`AbsDesktop-win-Setup.exe`) and a portable zip that update themselves. Settings has an Updates section; a downloaded update is installed when the app restarts.
- The plain Windows zip stays for winget and Scoop and never updates itself.
- New Linux package: `.deb` for Ubuntu 24.04 and later and Debian 13.
- Each release has a source tarball that builds without git. A Scoop bucket installs the Windows zip.

## 0.1.0 (2026-10-25)

The first release.

- Log in to Audiobookshelf 2.26.0 or later with a password. Several servers and accounts can be connected at once and turned off without logging out.
- Browse libraries, series, collections and authors, search, and see the server's home shelves.
- Stream audiobooks with chapters, bookmarks, speed, volume and a sleep timer. Media keys work through SMTC on Windows and MPRIS on Linux.
- Download books, play them offline and send the listening time when the server is back. When another device moved the position meanwhile, the app asks which to keep.
- Progress follows other devices through realtime events.
- Read EPUB, PDF and CBZ files in a separate reader window. The position is saved to the server.
- Books that exist on several accounts are shown once, with a choice of which copy to play. Matches can be split or made by hand.
- Themes with their own widgets (Catppuccin, Audiobookshelf, Material, WinUI 3, Glass), plus fonts, text size, corner radius, density and borders. Themes can be imported and exported.
- A zip for Windows and an AppImage for Linux.
