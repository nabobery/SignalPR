use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityRules {
    pub max_severity: Option<String>,
    pub default_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub system_prompt: String,
    pub agent_type: String,
    #[serde(default)]
    pub severity_rules: Option<SeverityRules>,
    #[serde(default)]
    pub provider: Option<String>,
}
