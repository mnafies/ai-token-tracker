use crate::db::Db;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Row key in the `settings` table holding the JSON-encoded [`Settings`].
const KEY: &str = "app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Currency {
    Usd,
    Idr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub currency: Currency,
    pub idr_rate: Option<f64>,
    pub launch_at_login: bool,
    pub process_poll_secs: u64,
    pub scan_roots: Vec<String>,
}

impl Settings {
    pub fn defaults(home: &Path) -> Settings {
        Settings {
            currency: Currency::Usd,
            idr_rate: None,
            launch_at_login: true,
            process_poll_secs: 5,
            scan_roots: vec![home.to_string_lossy().into_owned()],
        }
    }

    /// The saved settings: `Ok(None)` when nothing has been saved yet, `Err` when the saved
    /// value cannot be read.
    pub fn stored(db: &Db) -> anyhow::Result<Option<Settings>> {
        let raw: Option<String> = db
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [KEY], |r| r.get(0))
            .optional()?;
        Ok(raw.map(|v| serde_json::from_str(&v)).transpose()?)
    }

    /// Saved settings, falling back to defaults when none are saved or they are unreadable.
    pub fn load(db: &Db, home: &Path) -> Settings {
        match Self::stored(db) {
            Ok(Some(s)) => s,
            Ok(None) => Self::defaults(home),
            Err(e) => {
                eprintln!("settings unreadable, using defaults: {e:#}");
                Self::defaults(home)
            }
        }
    }

    pub fn save(&self, db: &Db) -> anyhow::Result<()> {
        db.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![KEY, serde_json::to_string(self)?],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_values() {
        let s = Settings::defaults(Path::new("/Users/x"));
        assert_eq!(s.currency, Currency::Usd);
        assert_eq!(s.idr_rate, None);
        assert!(s.launch_at_login);
        assert_eq!(s.process_poll_secs, 5);
        assert_eq!(s.scan_roots, vec!["/Users/x".to_string()]);
    }

    #[test]
    fn settings_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let home = Path::new("/Users/x");
        assert_eq!(Settings::stored(&db).unwrap(), None);
        assert_eq!(Settings::load(&db, home), Settings::defaults(home));
        let s = Settings {
            currency: Currency::Idr,
            idr_rate: Some(16000.0),
            launch_at_login: false,
            process_poll_secs: 10,
            scan_roots: vec!["/a".into(), "/b".into()],
        };
        s.save(&db).unwrap();
        assert_eq!(Settings::stored(&db).unwrap(), Some(s.clone()));
        assert_eq!(Settings::load(&db, home), s);
    }

    #[test]
    fn settings_json_shape() {
        let s = Settings::defaults(Path::new("/h"));
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"currency":"usd","idrRate":null,"launchAtLogin":true,"processPollSecs":5,"scanRoots":["/h"]}"#
        );
    }

    #[test]
    fn unreadable_settings_fall_back_to_defaults() {
        let db = Db::open_in_memory().unwrap();
        db.conn.execute("INSERT INTO settings(key, value) VALUES ('app', 'not json')", []).unwrap();
        assert!(Settings::stored(&db).is_err(), "unreadable is not the same as absent");
        assert_eq!(Settings::load(&db, Path::new("/h")), Settings::defaults(Path::new("/h")));
    }
}
