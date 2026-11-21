use crate::query::{DailyPoint, SessionRow};
use chrono::{DateTime, Local};

/// Quotes a CSV field when it contains a comma, quote or line break (RFC 4180).
fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Local time with offset, e.g. `2026-11-20T08:05:00+07:00`; empty when unknown.
fn time(ms: Option<i64>) -> String {
    ms.and_then(DateTime::from_timestamp_millis)
        .map(|t| {
            t.with_timezone(&Local)
                .format("%Y-%m-%dT%H:%M:%S%:z")
                .to_string()
        })
        .unwrap_or_default()
}

/// Cost cell; empty when the cost is all unpriced, so it never reads as $0.
fn cost(usd: f64, unpriced_tokens: i64) -> String {
    if usd == 0.0 && unpriced_tokens > 0 {
        String::new()
    } else {
        format!("{usd:.6}")
    }
}

pub fn sessions_csv(rows: &[SessionRow]) -> String {
    let mut out = String::from(
        "agent,session_id,title,project,started,updated,models,tokens,cost_usd,unpriced_tokens\n",
    );
    for r in rows {
        let cols = [
            field(&r.agent),
            field(&r.session_id),
            field(r.title.as_deref().unwrap_or("")),
            field(r.project_key.as_deref().unwrap_or("")),
            time(r.started_ms),
            time(r.updated_ms),
            field(&r.models.join(";")),
            r.tokens.to_string(),
            cost(r.cost_usd, r.unpriced_tokens),
            r.unpriced_tokens.to_string(),
        ];
        out.push_str(&cols.join(","));
        out.push('\n');
    }
    out
}

pub fn daily_csv(points: &[DailyPoint]) -> String {
    let mut out = String::from("date,agent,cost_usd,tokens,unpriced_tokens\n");
    for p in points {
        for a in &p.by_agent {
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                field(&p.date),
                field(&a.agent),
                cost(a.cost_usd, a.unpriced_tokens),
                a.tokens,
                a.unpriced_tokens
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::AgentCost;

    fn row(title: &str) -> SessionRow {
        SessionRow {
            agent: "claude-code".into(),
            session_id: "s1".into(),
            title: Some(title.into()),
            project_key: Some("/p/demo".into()),
            started_ms: Some(0),
            updated_ms: None,
            models: vec!["a".into(), "b".into()],
            tokens: 1234,
            cost_usd: 1.5,
            unpriced_tokens: 7,
            token_status: "recorded".into(),
        }
    }

    #[test]
    fn sessions_csv_escapes_commas_and_quotes() {
        let csv = sessions_csv(&[row(r#"He said "hi", ok"#)]);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "agent,session_id,title,project,started,updated,models,tokens,cost_usd,unpriced_tokens"
        );
        let line = lines.next().unwrap();
        assert!(
            line.starts_with(r#"claude-code,s1,"He said ""hi"", ok",/p/demo,"#),
            "{line}"
        );
        assert!(line.ends_with(",,a;b,1234,1.500000,7"), "{line}");
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn sessions_csv_quotes_newlines_and_empty_title() {
        let mut r = row("a\nb");
        let csv = sessions_csv(&[r.clone()]);
        assert!(csv.contains("\"a\nb\""));
        r.title = None;
        let csv = sessions_csv(&[r]);
        assert!(
            csv.lines()
                .nth(1)
                .unwrap()
                .starts_with("claude-code,s1,,/p/demo,")
        );
    }

    #[test]
    fn daily_csv_one_row_per_agent_per_day() {
        let ac = |agent: &str, cost: f64, tokens: i64, unpriced: i64| AgentCost {
            agent: agent.into(),
            cost_usd: cost,
            tokens,
            unpriced_tokens: unpriced,
        };
        let points = vec![
            DailyPoint {
                date: "2026-11-19".into(),
                by_agent: vec![ac("claude-code", 1.25, 100, 0), ac("codex", 0.5, 50, 20)],
            },
            DailyPoint {
                date: "2026-11-20".into(),
                by_agent: vec![],
            },
            DailyPoint {
                date: "2026-11-21".into(),
                by_agent: vec![ac("codex", 2.0, 10, 0), ac("gemini", 0.0, 40, 40)],
            },
        ];
        assert_eq!(
            daily_csv(&points),
            "date,agent,cost_usd,tokens,unpriced_tokens\n\
             2026-11-19,claude-code,1.250000,100,0\n\
             2026-11-19,codex,0.500000,50,20\n\
             2026-11-21,codex,2.000000,10,0\n\
             2026-11-21,gemini,,40,40\n"
        );
    }

    #[test]
    fn sessions_csv_leaves_cost_empty_when_only_unpriced() {
        let mut r = row("t");
        r.cost_usd = 0.0;
        let csv = sessions_csv(&[r.clone()]);
        assert!(csv.lines().nth(1).unwrap().ends_with(",1234,,7"), "{csv}");
        r.unpriced_tokens = 0;
        let csv = sessions_csv(&[r]);
        assert!(
            csv.lines().nth(1).unwrap().ends_with(",1234,0.000000,0"),
            "{csv}"
        );
    }
}
