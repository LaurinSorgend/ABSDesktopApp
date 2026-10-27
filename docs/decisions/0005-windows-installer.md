# 0005: Windows installer and updates

Status: accepted

## Decision

The Windows installer, its portable zip and the delta updates are made with Velopack (`vpk` 1.2.161, `velopack` crate 1.2.161). The app checks GitHub Releases through `velopack::sources::GithubSource`. The plain zip without an updater stays as the package for winget and Scoop.

## Reasons

- Velopack is active: release 1.2.161 on 2026-09-29 and commits in the week of this decision. cargo-packager's last release is 0.11.8 (2025-11-27), and `cargo-packager-updater` 0.2.3 is from 2025-07-21.
- Velopack makes delta packages; cargo-packager's updater downloads the whole package. A full package here is about 70 MB, almost all of it libmpv.
- One `vpk pack` call gives `Setup.exe`, `Portable.zip`, the full and delta `.nupkg` and the feed (`releases.win.json`). The installer is per user, needs no admin rights and installs to `%LocalAppData%\AbsDesktop`.
- The crate is small to use: `VelopackApp::build().run()` at the start of `main`, then `UpdateManager` with `check_for_updates`, `download_updates` and `wait_exit_then_apply_updates`. A downloaded update is also applied on the next start.
- cargo-packager would also give `.deb`, AppImage and PKGBUILD output, but those already have their own scripts, and Linux builds don't self-update.

## Consequences

- `vpk` is a .NET tool and needs a .NET SDK (8 or later, with `DOTNET_ROLL_FORWARD=Major` on newer SDKs). CI installs it with `dotnet tool install`. Keep the `vpk` version equal to the crate version.
- Only builds with the `self-update` Cargo feature contain the updater (`scripts/package-velopack.ps1`). `scripts/package-windows.ps1` without `-SelfUpdate` builds the zip for package managers, so winget and Scoop installs never update themselves.
- `ABS_UPDATE_FEED` points the updater at a local `vpk pack` output folder instead of GitHub, for testing an update without a release.
- Updates come from published releases only; the release workflow leaves drafts, so nothing reaches users before a release is published by hand. The repository has to be public for `GithubSource` without a token.
- Code signing (SignPath) is not set up yet. `vpk pack` signs through `--signTemplate`, which runs a command per file; SignPath's GitHub action signs an uploaded artifact instead, so signing will likely need a pass before `vpk pack` (the exes and DLLs) and another for `Setup.exe`.
- Tested by hand on Windows 11: a portable 0.1.0 found 0.1.1 in a local feed, downloaded it and applied it on the next start. The "Restart now" button and the installer itself were not tried.
