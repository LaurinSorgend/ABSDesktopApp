//! Reading: opens books in the reader process and saves where the user is.

use std::time::Duration;

use abs_core::EbookLocation;
use abs_core::ebooks::EbookOpen;
use abs_core::engine::Command;
use abs_reader_ipc::{FromReader, Location, OpenBook, Settings, ToReader};

use super::{App, with_app};
use crate::reader::ReaderProcess;

/// Setting key of the reader settings, as JSON. The same for every book.
const SETTINGS_KEY: &str = "reader_settings";
/// How long the position has to stay before it is saved.
const SAVE_DELAY: Duration = Duration::from_secs(5);

/// The reader process and the book it shows.
#[derive(Default)]
pub struct Reading {
    process: Option<ReaderProcess>,
    /// Counts started processes, so messages of an earlier one are ignored.
    generation: u64,
    /// Account and item of the open book.
    book: Option<(String, String)>,
    /// The latest position not saved yet.
    pending: Option<Location>,
    timer: slint::Timer,
}

impl App {
    /// From the item page: find the book, then [`App::ebook_ready`] opens it.
    pub fn read_item(&mut self, item_id: &str) {
        if let Some(account_id) = self.item_account(item_id) {
            self.engine.send(Command::OpenEbook {
                account_id,
                item_id: item_id.to_owned(),
            });
        }
    }

    pub(super) fn ebook_ready(&mut self, open: EbookOpen) {
        self.save_reading();
        let book = ToReader::Open(OpenBook {
            path: open.path,
            format: open.format,
            title: open.title,
            location: open.location,
            settings: self.reader_settings(),
        });
        if let Err(e) = self.send_to_reader(&book) {
            let text = self.ui(|_, s| s.invoke_reader_failed(e.to_string().into()));
            self.show_error(text.unwrap_or_default().into());
            return;
        }
        self.reading.book = Some((open.account_id, open.item_id));
    }

    pub(super) fn ebook_failed(&mut self, message: &str) {
        let text = self.ui(|_, s| s.invoke_reader_failed(message.into()));
        self.show_error(text.unwrap_or_default().into());
    }

    /// Sends to the running reader, starting one when there is none.
    fn send_to_reader(&mut self, message: &ToReader) -> std::io::Result<()> {
        let running = self
            .reading
            .process
            .as_mut()
            .is_some_and(|p| p.is_running());
        if !running {
            self.reading.generation += 1;
            let generation = self.reading.generation;
            let process = ReaderProcess::spawn(move |message| {
                let _ = slint::invoke_from_event_loop(move || {
                    with_app(move |app| app.reader_message(generation, message))
                });
            })?;
            self.reading.process = Some(process);
        }
        let process = self.reading.process.as_mut().expect("reader started");
        process.send(message)
    }

    fn reader_message(&mut self, generation: u64, message: FromReader) {
        if generation != self.reading.generation {
            return;
        }
        match message {
            FromReader::Relocate(location) => {
                self.reading.pending = Some(location);
                self.reading
                    .timer
                    .start(slint::TimerMode::SingleShot, SAVE_DELAY, || {
                        with_app(|app| app.save_reading())
                    });
            }
            FromReader::Settings { settings } => {
                let json = abs_reader_ipc::encode(&settings.clamped());
                let _ = self.db.store().settings().set(SETTINGS_KEY, json);
            }
            FromReader::Error { message } => eprintln!("reader: {message}"),
            FromReader::Closed => {
                self.save_reading();
                self.reading.book = None;
                self.reading.process = None;
            }
            FromReader::Ready | FromReader::Toc { .. } => {}
        }
    }

    /// Sends the pending position to the engine.
    pub(super) fn save_reading(&mut self) {
        self.reading.timer.stop();
        let pending = self.reading.pending.take();
        let (Some(location), Some((account_id, item_id))) = (pending, &self.reading.book) else {
            return;
        };
        if let Some((location, progress)) = server_position(&location) {
            self.engine.send(Command::SaveEbookProgress {
                account_id: account_id.clone(),
                item_id: item_id.clone(),
                location,
                progress,
            });
        }
    }

    /// Saves the position and closes the reader, as when the app quits.
    pub(super) fn close_reader(&mut self) {
        self.save_reading();
        self.reading.process = None;
    }

    fn reader_settings(&self) -> Settings {
        let json = self.db.store().settings().get(SETTINGS_KEY).ok().flatten();
        json.and_then(|j| abs_reader_ipc::decode::<Settings>(&j).ok())
            .unwrap_or_default()
            .clamped()
    }
}

/// What the web reader stores for a position: the CFI and its fraction, or the page
/// and `(page - 1) / pages` for PDF and comics.
fn server_position(location: &Location) -> Option<(EbookLocation, f64)> {
    if let Some(page) = location.page.filter(|p| *p > 0) {
        let progress = match location.pages {
            Some(pages) if pages > 0 => f64::from(page - 1) / f64::from(pages),
            _ => location.fraction,
        };
        return Some((EbookLocation::Page(page), progress.clamp(0.0, 1.0)));
    }
    let cfi = location.cfi.clone().filter(|c| c.starts_with("epubcfi("))?;
    Some((EbookLocation::Cfi(cfi), location.fraction.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_match_the_web_reader() {
        let page = Location {
            page: Some(5),
            pages: Some(24),
            fraction: 0.9,
            ..Location::default()
        };
        let (location, progress) = server_position(&page).unwrap();
        assert_eq!(location, EbookLocation::Page(5));
        assert!((progress - 4.0 / 24.0).abs() < 1e-9);
        let cfi = Location {
            cfi: Some("epubcfi(/6/4!/4/2/1:0)".into()),
            fraction: 0.25,
            ..Location::default()
        };
        let (location, progress) = server_position(&cfi).unwrap();
        assert_eq!(
            location,
            EbookLocation::Cfi("epubcfi(/6/4!/4/2/1:0)".into())
        );
        assert_eq!(progress, 0.25);
        let fake = Location {
            cfi: Some("not a cfi".into()),
            ..Location::default()
        };
        assert_eq!(server_position(&fake), None);
    }
}
