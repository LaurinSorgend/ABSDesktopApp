//! Times as the UI shows them.

/// `1:02:03` or `2:03`.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `-1:02:03`, for the time left.
pub fn remaining(seconds: f64) -> String {
    format!("-{}", clock(seconds))
}

/// Whole hours and the minutes after them, rounded to the nearest minute.
pub fn hours_minutes(seconds: f64) -> (i32, i32) {
    let minutes = (seconds.max(0.0) / 60.0).round() as i32;
    (minutes / 60, minutes % 60)
}

/// A speed as `1.25×`.
pub fn speed(speed: f64) -> String {
    let text = format!("{speed:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(clock(3723.9), "1:02:03");
        assert_eq!(clock(59.0), "0:59");
        assert_eq!(remaining(61.0), "-1:01");
        assert_eq!(hours_minutes(3.0 * 3600.0 + 29.6 * 60.0), (3, 30));
        assert_eq!(speed(1.0), "1×");
        assert_eq!(speed(1.25), "1.25×");
        assert_eq!(speed(1.5), "1.5×");
    }
}
