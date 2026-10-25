//! The `abs-reader` helper process: started for the first book, reused for later ones.

use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use abs_reader_ipc::{FromReader, ToReader};

/// A running reader and the pipe to its stdin.
pub struct ReaderProcess {
    child: Child,
    stdin: ChildStdin,
}

impl ReaderProcess {
    /// Starts the reader. `on_message` runs on a background thread for every line it
    /// prints, and once with `Closed` when its output ends.
    pub fn spawn(on_message: impl Fn(FromReader) + Send + 'static) -> io::Result<Self> {
        let mut command = Command::new(executable());
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        hide_console(&mut command);
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        std::thread::Builder::new()
            .name("reader-stdout".into())
            .spawn(move || read_messages(stdout, on_message))?;
        Ok(Self { child, stdin })
    }

    pub fn send(&mut self, message: &ToReader) -> io::Result<()> {
        writeln!(self.stdin, "{}", abs_reader_ipc::encode(message))?;
        self.stdin.flush()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for ReaderProcess {
    fn drop(&mut self) {
        let _ = self.send(&ToReader::Close);
    }
}

fn read_messages(stdout: impl io::Read, on_message: impl Fn(FromReader)) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        match abs_reader_ipc::decode::<FromReader>(&line) {
            Ok(message) => on_message(message),
            Err(_) => eprintln!("reader: {line}"),
        }
    }
    on_message(FromReader::Closed);
}

/// `ABS_READER_PATH`, else `abs-reader` next to this executable.
fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("ABS_READER_PATH") {
        return path.into();
    }
    let name = format!("abs-reader{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&name)))
        .unwrap_or_else(|| name.into())
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}
