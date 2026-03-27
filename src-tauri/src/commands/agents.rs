use serde::Serialize;

use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::queries;

const CUSTOM_AGENT_PREFIX: &str = "custom_agent_";

#[derive(Debug, Serialize, Clone)]
pub struct AgentDefinitionDto {
    pub name: String,
    pub system_prompt: String,
    pub agent_type: String,
    pub provider: Option<String>,
    pub is_builtin: bool,
}

#[derive(Debug, Serialize)]
pub struct AgentDefinitionsResponse {
    pub agents: Vec<AgentDefinitionDto>,
}

#[derive(Debug, serde::Deserialize)]
struct StoredAgentDefinition {
    name: String,
    system_prompt: String,
    agent_type: String,
    #[serde(default)]
    provider: Option<String>,
}

fn builtin_agents() -> Vec<AgentDefinitionDto> {
    vec![
        AgentDefinitionDto {
            name: "Security".into(),
            system_prompt: "You are a security-focused code reviewer. Identify vulnerabilities, injection risks, authentication issues, and insecure patterns.".into(),
            agent_type: "security".into(),
            provider: None,
            is_builtin: true,
        },
        AgentDefinitionDto {
            name: "Architecture".into(),
            system_prompt: "You are an architecture-focused code reviewer. Identify design issues, coupling problems, SOLID violations, and structural concerns.".into(),
            agent_type: "architecture".into(),
            provider: None,
            is_builtin: true,
        },
        AgentDefinitionDto {
            name: "Performance".into(),
            system_prompt: "You are a performance-focused code reviewer. Identify bottlenecks, N+1 queries, memory leaks, and inefficient algorithms.".into(),
            agent_type: "performance".into(),
            provider: None,
            is_builtin: true,
        },
    ]
}

#[tauri::command]
pub async fn get_agent_definitions(
    db: tauri::State<'_, AppDb>,
) -> Result<AgentDefinitionsResponse, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let mut agents = builtin_agents();

    let custom_entries = queries::get_settings_by_prefix(&conn, CUSTOM_AGENT_PREFIX)?;
    for (_key, value) in custom_entries {
        match serde_json::from_str::<StoredAgentDefinition>(&value) {
            Ok(parsed) => agents.push(AgentDefinitionDto {
                name: parsed.name,
                system_prompt: parsed.system_prompt,
                agent_type: parsed.agent_type,
                provider: parsed.provider,
                is_builtin: false,
            }),
            Err(e) => tracing::warn!("Skipping malformed custom agent JSON: {}", e),
        }
    }

    agents.sort_by(|a, b| {
        a.is_builtin
            .cmp(&b.is_builtin)
            .reverse()
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(AgentDefinitionsResponse { agents })
}

#[tauri::command]
pub async fn save_agent_definition(
    name: String,
    system_prompt: String,
    agent_type: String,
    provider: Option<String>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::InvalidInput("Agent name is required".into()));
    }
    if system_prompt.trim().is_empty() {
        return Err(AppError::InvalidInput("System prompt is required".into()));
    }

    let key = format!(
        "{}{}",
        CUSTOM_AGENT_PREFIX,
        name.to_lowercase().replace(' ', "_")
    );
    let value = serde_json::json!({
        "name": name,
        "system_prompt": system_prompt,
        "agent_type": agent_type,
        "provider": provider,
    });

    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::upsert_setting(&conn, &key, &value.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_agent_definition(
    name: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let key = format!(
        "{}{}",
        CUSTOM_AGENT_PREFIX,
        name.to_lowercase().replace(' ', "_")
    );
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let deleted = queries::delete_setting(&conn, &key)?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "Agent definition '{}' not found",
            name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_agents_count() {
        let agents = builtin_agents();
        assert_eq!(agents.len(), 3);
        assert!(agents.iter().all(|a| a.is_builtin));
    }

    #[test]
    fn test_builtin_agent_types() {
        let agents = builtin_agents();
        let types: Vec<&str> = agents.iter().map(|a| a.agent_type.as_str()).collect();
        assert!(types.contains(&"security"));
        assert!(types.contains(&"architecture"));
        assert!(types.contains(&"performance"));
    }
}
