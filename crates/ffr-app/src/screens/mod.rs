//! The six screens. Each has `fn ui(app: &mut FfrApp, ui: &mut egui::Ui)`.

pub mod about;
pub mod recover;
pub mod results;
pub mod scan;
pub mod settings;
pub mod source;

/// "1 h 02 m", "3 m 20 s", "12 s".
pub fn fmt_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{} h {:02} m", s / 3600, (s / 60) % 60)
    } else if s >= 60 {
        format!("{} m {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

/// Unix seconds to "YYYY-MM-DD HH:MM" (UTC).
pub fn fmt_time(t: i64) -> String {
    let (y, m, d) = ffr_core::util::civil_from_days(t.div_euclid(86_400));
    let r = t.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", r / 3600, (r / 60) % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(
            fmt_duration(std::time::Duration::from_secs(3725)),
            "1 h 02 m"
        );
        assert_eq!(
            fmt_duration(std::time::Duration::from_secs(200)),
            "3 m 20 s"
        );
        assert_eq!(fmt_time(0), "1970-01-01 00:00");
    }
}
