use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::ProviderError;
use crate::providers::traits::CodexReviewOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfigOptionValue {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfigOptionDescriptor {
    pub id: String,
    pub name: String,
    pub option_type: String,
    pub current_value: Option<String>,
    pub options: Vec<AcpConfigOptionValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcpSessionCapabilities {
    pub list: bool,
    pub load: bool,
    pub resume: bool,
    pub close: bool,
}

pub fn build_review_prompt(system_prompt: &str, output_schema: &str, diff: &str) -> String {
    format!(
        "{system}\n\n\
         ## Output format\n\
         You MUST respond with a single JSON object matching this schema. \
         Do not include any prose before or after the JSON. Do not wrap it \
         in markdown code fences. Output only the JSON object:\n\n\
         {schema}\n\n\
         ## Diff to review\n\
         {diff}",
        system = system_prompt,
        schema = output_schema,
        diff = diff
    )
}

pub fn parse_review_output(
    raw: &str,
    provider_label: &str,
) -> Result<CodexReviewOutput, ProviderError> {
    let trimmed = strip_code_fences(raw.trim());
    let json_slice = locate_json_object(trimmed).unwrap_or(trimmed);
    serde_json::from_str::<CodexReviewOutput>(json_slice).map_err(|e| match provider_label {
        "gemini" => ProviderError::GeminiFailed(format!(
            "Failed to parse review output as JSON: {} — raw text: {}",
            e,
            truncate_for_log(raw)
        )),
        "cursor" => ProviderError::CursorFailed(format!(
            "Failed to parse review output as JSON: {} — raw text: {}",
            e,
            truncate_for_log(raw)
        )),
        other => ProviderError::NotAvailable(format!(
            "Unsupported ACP provider '{}' for JSON parsing",
            other
        )),
    })
}

pub fn extract_available_modes(result: &Value) -> Vec<String> {
    if let Some(array) = result.get("modes").and_then(|v| v.as_array()) {
        return array
            .iter()
            .filter_map(|mode| mode.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }

    if let Some(available_modes) = result
        .get("modes")
        .and_then(|v| v.get("availableModes"))
        .and_then(|v| v.as_array())
    {
        return available_modes
            .iter()
            .filter_map(|mode| mode.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }

    extract_config_options(result)
        .into_iter()
        .find(|option| option.id == "mode")
        .map(|option| {
            option
                .options
                .into_iter()
                .map(|value| value.value)
                .collect()
        })
        .unwrap_or_default()
}

pub fn extract_config_options(result: &Value) -> Vec<AcpConfigOptionDescriptor> {
    result
        .get("configOptions")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(AcpConfigOptionDescriptor {
                id: option.get("id")?.as_str()?.to_string(),
                name: option
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Unnamed option")
                    .to_string(),
                option_type: option
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                current_value: option
                    .get("currentValue")
                    .and_then(|value| value.as_str())
                    .map(String::from),
                options: option
                    .get("options")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|candidate| {
                        Some(AcpConfigOptionValue {
                            value: candidate.get("value")?.as_str()?.to_string(),
                            label: candidate
                                .get("name")
                                .and_then(|value| value.as_str())
                                .unwrap_or_else(|| {
                                    candidate
                                        .get("value")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("unknown")
                                })
                                .to_string(),
                            description: candidate
                                .get("description")
                                .and_then(|value| value.as_str())
                                .map(String::from),
                        })
                    })
                    .collect(),
            })
        })
        .collect()
}

#[allow(dead_code)]
pub fn extract_session_capabilities(result: &Value) -> AcpSessionCapabilities {
    let session_caps = result
        .get("agentCapabilities")
        .and_then(|value| value.get("sessionCapabilities"));

    AcpSessionCapabilities {
        list: session_caps.and_then(|value| value.get("list")).is_some(),
        load: session_caps
            .and_then(|value| value.get("load"))
            .or_else(|| session_caps.and_then(|value| value.get("loadSession")))
            .is_some(),
        resume: session_caps.and_then(|value| value.get("resume")).is_some(),
        close: session_caps.and_then(|value| value.get("close")).is_some(),
    }
}

pub fn pick_rejection_option_id(options: &Value) -> Option<String> {
    let arr = options.as_array()?;
    for preferred in ["reject_once", "reject_always"] {
        for opt in arr {
            if opt.get("kind").and_then(|v| v.as_str()) == Some(preferred) {
                if let Some(id) = opt.get("optionId").and_then(|v| v.as_str()) {
                    return Some(id.to_string());
                }
            }
        }
    }
    for opt in arr {
        if let Some(id) = opt.get("optionId").and_then(|v| v.as_str()) {
            if id.to_lowercase().contains("reject") {
                return Some(id.to_string());
            }
        }
    }
    None
}

pub fn normalize_request_id(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

pub fn locate_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

pub fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 500;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...(truncated)", &s[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_modes_from_stable_shape() {
        let value = json!({
            "modes": {
                "currentModeId": "ask",
                "availableModes": [
                    { "id": "ask" },
                    { "id": "plan" }
                ]
            }
        });
        assert_eq!(extract_available_modes(&value), vec!["ask", "plan"]);
    }

    #[test]
    fn extracts_config_options() {
        let value = json!({
            "configOptions": [
                {
                    "id": "mode",
                    "name": "Session Mode",
                    "type": "select",
                    "currentValue": "ask",
                    "options": [
                        { "value": "ask", "name": "Ask" },
                        { "value": "plan", "name": "Plan" }
                    ]
                }
            ]
        });
        let options = extract_config_options(&value);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "mode");
        assert_eq!(options[0].options.len(), 2);
    }
}
