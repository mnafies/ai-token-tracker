use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct OrModel {
    pub id: String,
    pub prompt: f64,
    pub completion: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

fn price(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key)?
        .as_str()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|p| p.is_finite())
}

/// Parses the OpenRouter `/models` body. Models whose prompt/completion price is
/// missing, unparseable or negative (e.g. "-1" for variable-price routers) are skipped.
pub fn parse_models_json(body: &str) -> Result<Vec<OrModel>> {
    let root: serde_json::Value = serde_json::from_str(body)?;
    let data = root
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow::anyhow!("OpenRouter response has no data array"))?;
    let mut out = Vec::new();
    for m in data {
        let Some(id) = m.get("id").and_then(|i| i.as_str()) else {
            continue;
        };
        let Some(pr) = m.get("pricing") else { continue };
        let (Some(prompt), Some(completion)) = (price(pr, "prompt"), price(pr, "completion"))
        else {
            continue;
        };
        if prompt < 0.0 || completion < 0.0 {
            continue;
        }
        out.push(OrModel {
            id: id.to_string(),
            prompt,
            completion,
            cache_read: price(pr, "input_cache_read"),
            cache_write: price(pr, "input_cache_write"),
        });
    }
    Ok(out)
}

pub fn fetch_models() -> Result<Vec<OrModel>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let body = client
        .get("https://openrouter.ai/api/v1/models")
        .header("User-Agent", "ai-token-tracker/0.1")
        .send()?
        .error_for_status()?
        .text()?;
    parse_models_json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openrouter_pricing_strings() {
        let body = r#"{"data":[{"id":"anthropic/claude-opus-5.5","pricing":{"prompt":"0.000005","completion":"0.000025","input_cache_read":"0.0000005","input_cache_write":"0.00000625"}}]}"#;
        let m = parse_models_json(body).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "anthropic/claude-opus-5.5");
        assert_eq!(m[0].prompt, 5e-6);
        assert_eq!(m[0].completion, 25e-6);
        assert_eq!(m[0].cache_read, Some(5e-7));
        assert_eq!(m[0].cache_write, Some(6.25e-6));
    }

    #[test]
    fn missing_cache_prices_are_none() {
        let body = r#"{"data":[{"id":"openai/gpt-x","pricing":{"prompt":"0.000001","completion":"0.000002"}}]}"#;
        let m = parse_models_json(body).unwrap();
        assert_eq!(m[0].cache_read, None);
        assert_eq!(m[0].cache_write, None);
    }

    #[test]
    fn skips_variable_and_unparseable_prices() {
        let body = r#"{"data":[
          {"id":"openrouter/auto","pricing":{"prompt":"-1","completion":"-1"}},
          {"id":"a/b","pricing":{"prompt":"x","completion":"0.1"}},
          {"id":"a/c","pricing":{"completion":"0.1"}},
          {"id":"a/d","pricing":{"prompt":"0.1","completion":"0.2","input_cache_read":"zzz"}}]}"#;
        let m = parse_models_json(body).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "a/d");
        assert_eq!(m[0].cache_read, None);
    }
}
