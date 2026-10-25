use std::io::{BufRead, Write};

use abs_reader_ipc::{FromReader, ToReader};
use tao::event_loop::EventLoopProxy;

/// What the event loop wakes up for besides window events.
#[derive(Debug)]
pub enum UserEvent {
    Message(ToReader),
    /// The main app closed our stdin: it quit or wants us gone.
    StdinClosed,
}

/// Reads messages from stdin on its own thread. Lines that do not parse are reported
/// as errors and skipped.
pub fn spawn_stdin_reader(proxy: EventLoopProxy<UserEvent>) {
    std::thread::Builder::new()
        .name("reader-stdin".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match abs_reader_ipc::decode::<ToReader>(&line) {
                    Ok(message) => {
                        if proxy.send_event(UserEvent::Message(message)).is_err() {
                            return;
                        }
                    }
                    Err(e) => send(&FromReader::Error {
                        message: format!("bad message from the app: {e}"),
                    }),
                }
            }
            let _ = proxy.send_event(UserEvent::StdinClosed);
        })
        .expect("spawn stdin thread");
}

/// Writes one message to stdout.
pub fn send(message: &FromReader) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", abs_reader_ipc::encode(message));
    let _ = out.flush();
}

/// Passes on a message from the page after checking that it is one we know.
pub fn forward_from_page(body: &str) {
    match abs_reader_ipc::decode::<FromReader>(body) {
        Ok(message) => send(&message),
        Err(e) => send(&FromReader::Error {
            message: format!("bad message from the page: {e}"),
        }),
    }
}
