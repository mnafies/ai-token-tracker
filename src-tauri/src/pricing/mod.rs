pub mod matcher;
pub mod openrouter;

use crate::db::Db;
use anyhow::Result;
use openrouter::OrModel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingStatus {
    pub fetched_ms: Option<i64>,
    pub last_error: Option<String>,
    pub catalog_size: usize,
    pub unknown_models: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOverride {
    pub provider_hint: String,
    pub raw_model: String,
    pub action: String,
    pub openrouter_id: Option<String>,
    pub prompt: Option<f64>,
    pub completion: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

type Prices = (Option<f64>, Option<f64>, Option<f64>, Option<f64>);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn store_catalog(db: &Db, models: &[OrModel], now: i64) -> Result<()> {
    let tx = db.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM prices", [])?;
    {
        let mut st = tx.prepare(
            "INSERT OR REPLACE INTO prices(openrouter_id,prompt,completion,cache_read,cache_write,fetched_ms) VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        for m in models {
            st.execute(rusqlite::params![m.id, m.prompt, m.completion, m.cache_read, m.cache_write, now])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn refresh_prices(db: &Mutex<Db>, fetch: &dyn Fn() -> Result<Vec<OrModel>>) -> PricingStatus {
    let fetched = fetch();
    let db = db.lock().unwrap_or_else(|e| e.into_inner());
    let outcome = (|| -> Result<()> {
        match &fetched {
            Ok(models) => {
                let now = now_ms();
                store_catalog(&db, models, now)?;
                db.set_meta("prices_fetched_ms", &now.to_string())?;
                db.conn.execute("DELETE FROM app_meta WHERE key='prices_last_error'", [])?;
            }
            Err(e) => db.set_meta("prices_last_error", &e.to_string())?,
        }
        Ok(())
    })();
    let mut err = outcome.err().map(|e| e.to_string());
    if let Err(e) = rebuild_resolved(&db) {
        err.get_or_insert_with(|| e.to_string());
    }
    status(&db).unwrap_or_else(|e| PricingStatus {
        fetched_ms: None,
        last_error: Some(err.unwrap_or_else(|| e.to_string())),
        catalog_size: 0,
        unknown_models: vec![],
    })
}

pub fn rebuild_resolved(db: &Db) -> Result<()> {
    let catalog: HashMap<String, Prices> = {
        let mut st = db.conn.prepare("SELECT openrouter_id,prompt,completion,cache_read,cache_write FROM prices")?;
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, (Some(r.get(1)?), Some(r.get(2)?), r.get(3)?, r.get(4)?))))?
            .collect::<rusqlite::Result<_>>()?
    };
    let ids: Vec<String> = catalog.keys().cloned().collect();
    type Ov = (String, Option<String>, Prices);
    let overrides: HashMap<(String, String), Ov> = {
        let mut st = db.conn.prepare(
            "SELECT provider_hint,raw_model,action,openrouter_id,prompt,completion,cache_read,cache_write FROM model_overrides",
        )?;
        st.query_map([], |r| {
            Ok(((r.get(0)?, r.get(1)?), (r.get(2)?, r.get(3)?, (r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let models: Vec<(String, String)> = {
        let mut st = db.conn.prepare("SELECT DISTINCT provider_hint, model FROM events")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };

    let tx = db.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM resolved_prices", [])?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO resolved_prices(provider_hint,raw_model,status,openrouter_id,prompt,completion,cache_read,cache_write) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )?;
        for (hint, model) in &models {
            let none: Prices = (None, None, None, None);
            let (status, id, p): (&str, Option<String>, Prices) =
                match overrides.get(&(hint.clone(), model.clone())) {
                    Some((a, _, _)) if a == "ignore" => ("ignored", None, none),
                    Some((a, _, p)) if a == "manual" => ("manual", None, *p),
                    Some((a, Some(id), _)) if a == "map" => match catalog.get(id) {
                        Some(p) => ("mapped", Some(id.clone()), *p),
                        None => ("unknown", Some(id.clone()), none),
                    },
                    _ => match matcher::match_model(hint, model, &ids) {
                        Some(id) => {
                            let p = catalog[&id];
                            ("matched", Some(id), p)
                        }
                        None => ("unknown", None, none),
                    },
                };
            ins.execute(rusqlite::params![hint, model, status, id, p.0, p.1, p.2, p.3])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn set_override(db: &Db, o: &ModelOverride) -> Result<()> {
    if o.action == "clear" {
        db.conn.execute(
            "DELETE FROM model_overrides WHERE provider_hint=?1 AND raw_model=?2",
            rusqlite::params![o.provider_hint, o.raw_model],
        )?;
    } else {
        db.conn.execute(
            "INSERT OR REPLACE INTO model_overrides(provider_hint,raw_model,action,openrouter_id,prompt,completion,cache_read,cache_write) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![o.provider_hint, o.raw_model, o.action, o.openrouter_id, o.prompt, o.completion, o.cache_read, o.cache_write],
        )?;
    }
    rebuild_resolved(db)
}

pub fn status(db: &Db) -> Result<PricingStatus> {
    let fetched_ms = db.get_meta("prices_fetched_ms")?.and_then(|s| s.parse().ok());
    let last_error = db.get_meta("prices_last_error")?;
    let catalog_size: i64 = db.conn.query_row("SELECT COUNT(*) FROM prices", [], |r| r.get(0))?;
    let mut st = db.conn.prepare(
        "SELECT provider_hint, raw_model FROM resolved_prices WHERE status='unknown' ORDER BY provider_hint, raw_model",
    )?;
    let unknown_models = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(PricingStatus { fetched_ms, last_error, catalog_size: catalog_size as usize, unknown_models })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, p: f64, c: f64) -> OrModel {
        OrModel { id: id.into(), prompt: p, completion: c, cache_read: None, cache_write: None }
    }
    fn add_event(db: &Db, key: &str, hint: &str, m: &str) {
        db.conn
            .execute(
                "INSERT INTO events(event_key,agent,session_id,ts_ms,cwd,model,provider_hint,input,output,reasoning,cache_read,cache_write) VALUES (?1,'a','s',1,NULL,?2,?3,1,1,0,0,0)",
                rusqlite::params![key, m, hint],
            )
            .unwrap();
    }
    fn resolved(db: &Db, hint: &str, m: &str) -> (String, Option<String>, Option<f64>) {
        db.conn
            .query_row(
                "SELECT status, openrouter_id, prompt FROM resolved_prices WHERE provider_hint=?1 AND raw_model=?2",
                rusqlite::params![hint, m],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    #[test]
    fn default_ignores_synthetic_and_auto_review() {
        let db = Db::open_in_memory().unwrap();
        add_event(&db, "1", "anthropic", "<synthetic>");
        add_event(&db, "2", "openai", "codex-auto-review");
        rebuild_resolved(&db).unwrap();
        assert_eq!(resolved(&db, "anthropic", "<synthetic>").0, "ignored");
        assert_eq!(resolved(&db, "openai", "codex-auto-review").0, "ignored");
    }

    #[test]
    fn manual_override_wins_over_match() {
        let m = Mutex::new(Db::open_in_memory().unwrap());
        {
            let db = m.lock().unwrap();
            add_event(&db, "1", "anthropic", "claude-opus-5-5");
        }
        refresh_prices(&m, &|| Ok(vec![model("anthropic/claude-opus-5.5", 5e-6, 25e-6)]));
        let db = m.lock().unwrap();
        assert_eq!(resolved(&db, "anthropic", "claude-opus-5-5").0, "matched");
        set_override(&db, &ModelOverride {
            provider_hint: "anthropic".into(), raw_model: "claude-opus-5-5".into(), action: "manual".into(),
            openrouter_id: None, prompt: Some(1e-6), completion: Some(2e-6), cache_read: None, cache_write: None,
        }).unwrap();
        let r = resolved(&db, "anthropic", "claude-opus-5-5");
        assert_eq!(r.0, "manual");
        assert_eq!(r.2, Some(1e-6));
        set_override(&db, &ModelOverride {
            provider_hint: "anthropic".into(), raw_model: "claude-opus-5-5".into(), action: "clear".into(),
            openrouter_id: None, prompt: None, completion: None, cache_read: None, cache_write: None,
        }).unwrap();
        assert_eq!(resolved(&db, "anthropic", "claude-opus-5-5").0, "matched");
    }

    #[test]
    fn map_override_to_missing_id_is_unknown_and_keeps_id() {
        let db = Db::open_in_memory().unwrap();
        add_event(&db, "1", "openai", "weird");
        set_override(&db, &ModelOverride {
            provider_hint: "openai".into(), raw_model: "weird".into(), action: "map".into(),
            openrouter_id: Some("openai/nope".into()), prompt: None, completion: None, cache_read: None, cache_write: None,
        }).unwrap();
        let r = resolved(&db, "openai", "weird");
        assert_eq!(r.0, "unknown");
        assert_eq!(r.1.as_deref(), Some("openai/nope"));
    }

    #[test]
    fn refresh_failure_keeps_old_prices_and_reports_offline() {
        let m = Mutex::new(Db::open_in_memory().unwrap());
        {
            let db = m.lock().unwrap();
            add_event(&db, "1", "anthropic", "claude-opus-5-5");
            add_event(&db, "2", "openai", "mystery");
        }
        let s1 = refresh_prices(&m, &|| Ok(vec![model("anthropic/claude-opus-5.5", 5e-6, 25e-6)]));
        assert_eq!(s1.catalog_size, 1);
        assert!(s1.last_error.is_none());
        assert!(s1.fetched_ms.is_some());
        assert_eq!(s1.unknown_models, vec![("openai".to_string(), "mystery".to_string())]);
        let s2 = refresh_prices(&m, &|| Err(anyhow::anyhow!("offline")));
        assert_eq!(s2.catalog_size, 1);
        assert!(s2.last_error.as_deref().unwrap().contains("offline"));
        assert_eq!(s2.fetched_ms, s1.fetched_ms);
        let db = m.lock().unwrap();
        assert_eq!(resolved(&db, "anthropic", "claude-opus-5-5").0, "matched");
    }

    #[test]
    fn empty_db_with_failed_fetch_is_all_unknown_without_panic() {
        let m = Mutex::new(Db::open_in_memory().unwrap());
        add_event(&m.lock().unwrap(), "1", "openai", "gpt-9");
        let s = refresh_prices(&m, &|| Err(anyhow::anyhow!("offline")));
        assert_eq!(s.catalog_size, 0);
        assert_eq!(s.fetched_ms, None);
        assert!(s.last_error.is_some());
        assert_eq!(s.unknown_models.len(), 1);
    }
}
