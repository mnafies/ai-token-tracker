//! Dev tool: headless ingest of the real $HOME (read-only) into a throwaway DB.
//! Prints aggregates only (counts and token totals), never any content.
use ai_token_tracker_lib::{adapters, db::Db, ingest, pricing};
use std::sync::Mutex;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let home = std::path::PathBuf::from(std::env::var("HOME")?);
    let dir = std::env::temp_dir().join(format!("att-real-data-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let db = Mutex::new(Db::open(&dir.join("check.db"))?);

    let adapters = adapters::default_adapters(&home);
    let t = Instant::now();
    let report = ingest::ingest_all(&db, &adapters, &|_| {})?;
    let elapsed = t.elapsed();
    pricing::rebuild_resolved(&db.lock().unwrap())?;

    println!("ingest: {:?} (sources {}, events {}, sessions {}, errors {})",
        elapsed, report.sources_read, report.events, report.sessions, report.errors);
    {
    let d = db.lock().unwrap();
    println!("agent | sessions | events | input | output | cache_read | cache_write | reasoning");
    let mut st = d.conn.prepare(
        "SELECT agent, COUNT(DISTINCT session_id), COUNT(*), SUM(input), SUM(output), SUM(cache_read), SUM(cache_write), SUM(reasoning) \
         FROM events GROUP BY agent ORDER BY agent")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
        r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?)))?;
    for r in rows {
        let r = r?;
        println!("{} | {} | {} | {} | {} | {} | {} | {}", r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7);
    }
    println!("top 3 sessions by output tokens: (agent, session_id, events, output)");
    let mut st = d.conn.prepare(
        "SELECT agent, session_id, COUNT(*), SUM(output) o FROM events GROUP BY agent, session_id ORDER BY o DESC LIMIT 3")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?;
    for r in rows {
        let r = r?;
        println!("({}, {}, {}, {})", r.0, r.1, r.2, r.3);
    }
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
