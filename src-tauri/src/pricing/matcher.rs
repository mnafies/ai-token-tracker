pub fn normalize(s: &str) -> String {
    let mut t = s.to_lowercase();
    if t.ends_with(']') {
        if let Some(i) = t.rfind('[') {
            t.truncate(i);
        }
    }
    let t: String = t.chars().map(|c| if c == '.' || c == '_' { '-' } else { c }).collect();
    let b = t.as_bytes();
    if b.len() > 9 && b[b.len() - 9] == b'-' && b[b.len() - 8..].iter().all(u8::is_ascii_digit) {
        return t[..t.len() - 9].to_string();
    }
    t
}

/// Exact match after normalization, restricted to ids prefixed `{provider_hint}/`.
/// With several matches the lexicographically smallest id wins.
pub fn match_model(provider_hint: &str, raw_model: &str, catalog_ids: &[String]) -> Option<String> {
    let prefix = format!("{provider_hint}/");
    let want = normalize(raw_model);
    catalog_ids
        .iter()
        .filter(|id| id.strip_prefix(&prefix).is_some_and(|rest| normalize(rest) == want))
        .min()
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_dots_dates_and_brackets() {
        assert_eq!(normalize("claude-opus-5-5[1m]"), "claude-opus-5-5");
        assert_eq!(normalize("claude-sonnet-4-5-20250929"), "claude-sonnet-4-5");
        assert_eq!(normalize("gpt-5.6-luna"), "gpt-5-6-luna");
    }

    #[test]
    fn matches_claude_dash_version_to_dotted_id() {
        let ids = vec!["anthropic/claude-opus-5.5".to_string()];
        assert_eq!(match_model("anthropic", "claude-opus-5-5", &ids), Some("anthropic/claude-opus-5.5".to_string()));
    }

    #[test]
    fn does_not_match_fast_variant_to_base() {
        let ids = vec!["openai/gpt-5.6-luna".to_string()];
        assert_eq!(match_model("openai", "gpt-5.6-luna-fast", &ids), None);
    }

    #[test]
    fn respects_provider_prefix() {
        let ids = vec!["anthropic/claude-opus-5.5".to_string()];
        assert_eq!(match_model("openai", "claude-opus-5-5", &ids), None);
    }

    #[test]
    fn picks_lexicographically_smallest_on_multiple_matches() {
        let ids = vec!["anthropic/claude-x-1.0".to_string(), "anthropic/claude-x-1-0".to_string()];
        assert_eq!(match_model("anthropic", "claude-x-1-0", &ids), Some("anthropic/claude-x-1-0".to_string()));
    }
}
