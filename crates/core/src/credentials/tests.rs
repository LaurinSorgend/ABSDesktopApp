use abs_api::TokenSink;

use super::*;

fn round_trip(credentials: &Credentials, account: &str) {
    assert_eq!(credentials.load(account).unwrap(), None);
    credentials.save(account, "first").unwrap();
    credentials
        .sink(account)
        .store_refresh_token("second")
        .unwrap();
    assert_eq!(
        credentials.load(account).unwrap().as_deref(),
        Some("second")
    );
    credentials.delete(account).unwrap();
    credentials.delete(account).unwrap();
    assert_eq!(credentials.load(account).unwrap(), None);
}

#[test]
fn memory_backend() {
    let credentials = Credentials::memory();
    assert!(!credentials.is_persistent());
    round_trip(&credentials, "account-a");
}

#[test]
fn file_backend() {
    let dir = tempfile::tempdir().unwrap();
    let credentials = Credentials::files(dir.path().join("tokens"));
    round_trip(&credentials, "account-b");
}

#[cfg(unix)]
#[test]
fn token_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    Credentials::files(dir.path()).save("acc", "t").unwrap();
    let mode = std::fs::metadata(dir.path().join("acc.token"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// Touches the real OS keyring; run with `cargo test -p abs-core -- --ignored`.
#[test]
#[ignore]
fn system_keyring() {
    let credentials = Credentials::system().unwrap();
    round_trip(&credentials, &format!("test-{}", uuid::Uuid::new_v4()));
}
