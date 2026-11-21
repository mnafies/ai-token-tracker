use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    OpenCode,
    GeminiCli,
    CopilotCli,
    ClaudeDesktop,
    VsCode,
}

impl AgentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::OpenCode => "opencode",
            AgentKind::GeminiCli => "gemini-cli",
            AgentKind::CopilotCli => "copilot-cli",
            AgentKind::ClaudeDesktop => "claude-desktop",
            AgentKind::VsCode => "vs-code",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "claude-code" => AgentKind::ClaudeCode,
            "codex" => AgentKind::Codex,
            "opencode" => AgentKind::OpenCode,
            "gemini-cli" => AgentKind::GeminiCli,
            "copilot-cli" => AgentKind::CopilotCli,
            "claude-desktop" => AgentKind::ClaudeDesktop,
            "vs-code" => AgentKind::VsCode,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "opencode",
            AgentKind::GeminiCli => "Gemini CLI",
            AgentKind::CopilotCli => "Copilot CLI",
            AgentKind::ClaudeDesktop => "Claude Desktop",
            AgentKind::VsCode => "VS Code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenStatus {
    Recorded,
    NotRecorded,
}

impl TokenStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TokenStatus::Recorded => "recorded",
            TokenStatus::NotRecorded => "not-recorded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub agent: AgentKind,
    pub event_key: String,
    pub session_id: String,
    pub ts_ms: i64,
    pub cwd: Option<String>,
    pub model: String,
    pub provider_hint: String,
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub agent: AgentKind,
    pub session_id: String,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub started_ms: Option<i64>,
    pub updated_ms: Option<i64>,
    pub token_status: TokenStatus,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ParseOutput {
    pub events: Vec<UsageEvent>,
    pub sessions: Vec<SessionMeta>,
}
