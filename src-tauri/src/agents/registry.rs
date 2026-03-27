use super::definition::AgentDefinition;

pub struct AgentRegistry {
    definitions: Vec<AgentDefinition>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            definitions: vec![],
        }
    }

    /// Load definitions from config, validating each one.
    /// Invalid definitions (empty name or system_prompt) are skipped with a warning.
    pub fn load_from_config(custom_agents: &[AgentDefinition]) -> Self {
        let definitions: Vec<AgentDefinition> = custom_agents
            .iter()
            .filter(|def| {
                if def.name.trim().is_empty() {
                    tracing::warn!(
                        "Skipping custom agent with empty name (agent_type: {})",
                        def.agent_type
                    );
                    return false;
                }
                if def.agent_type.trim().is_empty() {
                    tracing::warn!("Skipping custom agent '{}' with empty agent_type", def.name);
                    return false;
                }
                if def.system_prompt.trim().is_empty() {
                    tracing::warn!(
                        "Skipping custom agent '{}' with empty system_prompt",
                        def.name
                    );
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        Self { definitions }
    }

    pub fn definitions(&self) -> &[AgentDefinition] {
        &self.definitions
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_def(name: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            system_prompt: format!("You are a {name}-focused reviewer."),
            agent_type: name.to_string(),
            severity_rules: None,
            provider: None,
        }
    }

    #[test]
    fn test_load_valid_definitions() {
        let defs = vec![make_valid_def("accessibility"), make_valid_def("i18n")];
        let registry = AgentRegistry::load_from_config(&defs);
        assert_eq!(registry.definitions().len(), 2);
        assert_eq!(registry.definitions()[0].name, "accessibility");
        assert_eq!(registry.definitions()[1].name, "i18n");
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_skip_invalid_missing_name() {
        let defs = vec![
            AgentDefinition {
                name: "".to_string(),
                system_prompt: "some prompt".to_string(),
                agent_type: "custom".to_string(),
                severity_rules: None,
                provider: None,
            },
            make_valid_def("valid"),
        ];
        let registry = AgentRegistry::load_from_config(&defs);
        assert_eq!(registry.definitions().len(), 1);
        assert_eq!(registry.definitions()[0].name, "valid");
    }

    #[test]
    fn test_skip_invalid_missing_prompt() {
        let defs = vec![AgentDefinition {
            name: "no-prompt".to_string(),
            system_prompt: "   ".to_string(),
            agent_type: "custom".to_string(),
            severity_rules: None,
            provider: None,
        }];
        let registry = AgentRegistry::load_from_config(&defs);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_empty_config() {
        let registry = AgentRegistry::load_from_config(&[]);
        assert!(registry.is_empty());
        assert_eq!(registry.definitions().len(), 0);
    }

    #[test]
    fn test_new_creates_empty_registry() {
        let registry = AgentRegistry::new();
        assert!(registry.is_empty());
    }
}
