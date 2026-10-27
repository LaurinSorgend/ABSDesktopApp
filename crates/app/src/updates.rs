//! Self-update through Velopack, in builds with the `self-update` feature on Windows.
//! Every other build has the same functions, which do nothing.

/// Setting: look for an update at start (0 off, 1 on).
pub const AUTO_CHECK: &str = "update_check";

/// What a check found.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    not(all(windows, feature = "self-update")),
    allow(dead_code, reason = "only Velopack builds find updates")
)]
pub enum Outcome {
    UpToDate,
    /// The update is downloaded; it is applied on restart.
    Ready(String),
    Failed(String),
}

#[cfg(all(windows, feature = "self-update"))]
mod velopack_updates {
    use velopack::sources::{FileSource, GithubSource, UpdateSource};
    use velopack::{UpdateCheck, UpdateManager, VelopackApp};

    use super::Outcome;

    const REPOSITORY: &str = "https://github.com/LaurinSorgend/ABSDesktopApp";
    /// A folder with a `vpk pack` output to update from instead of GitHub, for testing.
    const FEED_VAR: &str = "ABS_UPDATE_FEED";

    /// Runs Velopack's install, update and uninstall hooks. May exit or restart the process.
    pub fn startup() {
        VelopackApp::build().run();
    }

    /// True when the app was installed by the Velopack installer or portable zip.
    pub fn available() -> bool {
        manager().is_ok()
    }

    /// Looks for a newer release and downloads it. Blocks; call it off the UI thread.
    pub fn check() -> Outcome {
        match check_and_download() {
            Ok(Some(version)) => Outcome::Ready(version),
            Ok(None) => Outcome::UpToDate,
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }

    /// Starts the updater, which applies the downloaded update once this process has
    /// exited and then starts the app again. False when nothing is downloaded.
    pub fn apply_after_exit() -> Result<bool, String> {
        let manager = manager().map_err(|e| e.to_string())?;
        let Some(asset) = manager.get_update_pending_restart() else {
            return Ok(false);
        };
        manager
            .wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new())
            .map_err(|e| e.to_string())?;
        Ok(true)
    }

    fn check_and_download() -> Result<Option<String>, velopack::Error> {
        let manager = manager()?;
        if let Some(pending) = manager.get_update_pending_restart() {
            return Ok(Some(pending.Version));
        }
        let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? else {
            return Ok(None);
        };
        manager.download_updates(&update, None)?;
        Ok(Some(update.TargetFullRelease.Version.clone()))
    }

    fn manager() -> Result<UpdateManager, velopack::Error> {
        let source: Box<dyn UpdateSource> = match std::env::var_os(FEED_VAR) {
            Some(dir) => Box::new(FileSource::new(dir)),
            None => Box::new(GithubSource::new(REPOSITORY, None, false)),
        };
        UpdateManager::new_boxed(source, None, None)
    }
}

#[cfg(not(all(windows, feature = "self-update")))]
mod velopack_updates {
    use super::Outcome;

    pub fn startup() {}

    pub fn available() -> bool {
        false
    }

    pub fn check() -> Outcome {
        Outcome::UpToDate
    }

    pub fn apply_after_exit() -> Result<bool, String> {
        Ok(false)
    }
}

pub use velopack_updates::{apply_after_exit, available, check, startup};
