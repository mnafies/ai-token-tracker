use crate::db::Db;
use crate::model::AgentKind;
use serde::Serialize;
use rusqlite::OptionalExtension;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningAgent {
    pub agent: AgentKind,
    pub pid: u32,
    pub cwd: Option<String>,
    pub started_ms: i64,
    pub session_hint: Option<String>,
}

pub fn classify(name: &str, exe: &str, cmd: &[String]) -> Option<AgentKind> {
    if name.contains("Helper") {
        return None;
    }
    let cmd0 = cmd.first().map(String::as_str).unwrap_or("");
    let cmd1 = cmd.get(1).map(String::as_str).unwrap_or("");
    if name == "claude" || cmd0.ends_with("/claude") {
        return Some(AgentKind::ClaudeCode);
    }
    if name == "codex" || exe.contains("/Codex.app/") {
        return Some(AgentKind::Codex);
    }
    if name == "opencode" {
        return Some(AgentKind::OpenCode);
    }
    if cmd.iter().any(|a| a.contains("@google/gemini-cli")) || cmd1.ends_with("/gemini") {
        return Some(AgentKind::GeminiCli);
    }
    if cmd.iter().any(|a| a.contains("@github/copilot")) || name == "copilot" {
        return Some(AgentKind::CopilotCli);
    }
    if exe.contains("/Claude.app/Contents/MacOS/Claude") {
        return Some(AgentKind::ClaudeDesktop);
    }
    if exe.contains("/Visual Studio Code.app/Contents/MacOS/") {
        return Some(AgentKind::VsCode);
    }
    None
}

pub fn snapshot(sys: &mut System) -> Vec<RunningAgent> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::new()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
    let mut best: HashMap<(&'static str, Option<String>), RunningAgent> = HashMap::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().into_owned();
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cmd: Vec<String> = p
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let Some(agent) = classify(&name, &exe, &cmd) else {
            continue;
        };
        let cwd = p.cwd().map(|c| c.to_string_lossy().into_owned());
        let ra = RunningAgent {
            agent,
            pid: pid.as_u32(),
            cwd: cwd.clone(),
            started_ms: p.start_time() as i64 * 1000,
            session_hint: None,
        };
        match best.entry((agent.as_str(), cwd)) {
            Entry::Occupied(mut e) => {
                if ra.pid < e.get().pid {
                    e.insert(ra);
                }
            }
            Entry::Vacant(e) => {
                e.insert(ra);
            }
        }
    }
    let mut out: Vec<RunningAgent> = best.into_values().collect();
    out.sort_by(|a, b| (a.agent.as_str(), a.pid).cmp(&(b.agent.as_str(), b.pid)));
    out
}

pub fn attach_session_hints(db: &Db, running: &mut [RunningAgent]) -> anyhow::Result<()> {
    let mut stmt = db.conn.prepare(
        "SELECT session_id FROM sessions WHERE agent = ?1 AND cwd = ?2 ORDER BY updated_ms DESC LIMIT 1",
    )?;
    for r in running.iter_mut() {
        let Some(cwd) = r.cwd.as_deref() else {
            continue;
        };
        r.session_hint = stmt
            .query_row(rusqlite::params![r.agent.as_str(), cwd], |row| row.get(0))
            .optional()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn classifies_claude_cli() {
        assert_eq!(
            classify("claude", "/Users/x/.local/bin/claude", &s(&["claude"])),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn classifies_gemini_node() {
        assert_eq!(
            classify(
                "node",
                "/opt/homebrew/bin/node",
                &s(&["node", "/opt/homebrew/bin/gemini"])
            ),
            Some(AgentKind::GeminiCli)
        );
    }

    #[test]
    fn classifies_codex_desktop() {
        assert_eq!(
            classify("Codex", "/Applications/Codex.app/Contents/MacOS/Codex", &s(&["Codex"])),
            Some(AgentKind::Codex)
        );
    }

    #[test]
    fn ignores_helpers_and_unrelated() {
        assert_eq!(
            classify(
                "Claude Helper (Renderer)",
                "/Applications/Claude.app/Contents/Frameworks/Claude Helper (Renderer).app/Contents/MacOS/Claude Helper (Renderer)",
                &s(&["Claude Helper (Renderer)"])
            ),
            None
        );
        assert_eq!(classify("bash", "/bin/bash", &s(&["bash"])), None);
    }

    #[test]
    fn session_hint_matches_latest_session_in_same_cwd() {
        let db = Db::open_in_memory().unwrap();
        for (agent, sid, cwd, upd) in [
            ("claude-code", "old", "/p", 100),
            ("claude-code", "new", "/p", 200),
            ("claude-code", "other-cwd", "/q", 300),
            ("codex", "other-agent", "/p", 400),
        ] {
            db.conn
                .execute(
                    "INSERT INTO sessions(agent, session_id, cwd, title, started_ms, updated_ms, token_status) VALUES (?1, ?2, ?3, NULL, 0, ?4, 'recorded')",
                    rusqlite::params![agent, sid, cwd, upd],
                )
                .unwrap();
        }
        let mk = |cwd: Option<&str>| RunningAgent {
            agent: AgentKind::ClaudeCode,
            pid: 1,
            cwd: cwd.map(String::from),
            started_ms: 0,
            session_hint: None,
        };
        let mut running = vec![mk(Some("/p")), mk(Some("/none")), mk(None)];
        attach_session_hints(&db, &mut running).unwrap();
        assert_eq!(running[0].session_hint.as_deref(), Some("new"));
        assert_eq!(running[1].session_hint, None);
        assert_eq!(running[2].session_hint, None);
    }

    #[test]
    fn snapshot_does_not_panic() {
        let mut sys = System::new();
        let _ = snapshot(&mut sys);
    }
}
