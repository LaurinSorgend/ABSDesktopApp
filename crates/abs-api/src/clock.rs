//! The server's clock, read from the `Date` header of its answers.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Response;
use reqwest::header::DATE;

/// Server time minus local time in milliseconds, from the latest answer.
#[derive(Debug, Default)]
pub(crate) struct Clock(Mutex<Option<i64>>);

impl Clock {
    pub(crate) fn note(&self, response: &Response) {
        let header = response.headers().get(DATE).and_then(|v| v.to_str().ok());
        if let Some(offset) = header.and_then(|h| offset_from(h, local_ms())) {
            *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(offset);
        }
    }

    pub(crate) fn offset(&self) -> Option<i64> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Server time minus `local_ms` for an HTTP date such as `Sun, 25 Oct 2026 10:00:00 GMT`.
pub(crate) fn offset_from(date: &str, local_ms: i64) -> Option<i64> {
    let server = httpdate::parse_http_date(date).ok()?;
    let server_ms = server.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    Some(server_ms - local_ms)
}

fn local_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_from_date_header() {
        let server_ms = 1_792_900_000_000;
        let date =
            httpdate::fmt_http_date(UNIX_EPOCH + std::time::Duration::from_millis(server_ms));
        assert_eq!(offset_from(&date, server_ms as i64 - 90_000), Some(90_000));
        assert_eq!(offset_from("yesterday", 0), None);
    }
}
