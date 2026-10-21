//! Today's finished focus sessions, kept in `pomodoro-day.json` so a restart doesn't zero the count.
//! The file holds one local date and its count; a file from an earlier day counts as zero.

use crate::error::Result;
use crate::fsutil::write_atomic;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct DayFile {
    date: String,
    count: u32,
}

/// The count saved for `today`; zero when the file is missing, unreadable, or from another day.
pub fn load(path: &Path, today: NaiveDate) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<DayFile>(&text).ok())
        .filter(|f| f.date == today.to_string())
        .map_or(0, |f| f.count)
}

pub fn save(path: &Path, today: NaiveDate, count: u32) -> Result<()> {
    let file = DayFile { date: today.to_string(), count };
    write_atomic(path, serde_json::to_string(&file).unwrap_or_default().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hackatime-focus-day-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("pomodoro-day.json")
    }

    #[test]
    fn a_saved_count_comes_back_the_same_day() {
        let p = path("same");
        save(&p, day(21), 3).unwrap();
        assert_eq!(load(&p, day(21)), 3);
    }

    #[test]
    fn a_new_day_starts_at_zero() {
        let p = path("next");
        save(&p, day(21), 3).unwrap();
        assert_eq!(load(&p, day(22)), 0);
    }

    #[test]
    fn missing_or_corrupt_files_count_as_zero() {
        let p = path("bad");
        assert_eq!(load(&p, day(21)), 0);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load(&p, day(21)), 0);
    }
}
