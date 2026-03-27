use super::traits::ReviewInput;

pub const OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "findings": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "title": { "type": "string" },
          "body": { "type": "string" },
          "file_path": { "type": "string" },
          "line_start": { "type": "integer" },
          "line_end": { "type": "integer" },
          "severity": { "type": "string", "enum": ["blocker", "critical", "warning", "info", "nitpick"] },
          "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
          "evidence": { "type": "array", "items": { "type": "string" } },
          "agent_type": { "type": "string", "enum": ["security", "architecture", "performance"] }
        },
        "required": ["title", "body", "severity", "confidence", "agent_type"]
      }
    },
    "overall_assessment": { "type": "string" },
    "overall_confidence": { "type": "number" }
  },
  "required": ["findings"]
}"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum AgentFocus {
    General,
    Security,
    Architecture,
    Performance,
}

impl AgentFocus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Security => "security",
            Self::Architecture => "architecture",
            Self::Performance => "performance",
        }
    }
}

impl std::fmt::Display for AgentFocus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn base_review_prompt() -> String {
    r#"You are a code reviewer analyzing a pull request diff. Focus on:
1. Security: auth bypass, injection, IDOR, secret exposure, logic flaws
2. Architecture: boundary violations, coupling drift, design inconsistencies
3. Performance: inefficient loops, N+1 patterns, memory pressure, needless I/O

Rules:
- Only flag actionable issues introduced by this change
- Provide file path and line range for each finding
- Prioritize severe issues over nitpicks
- Assign a confidence score 0-1 for each finding
- Assign agent_type as "security", "architecture", or "performance"
- Assign severity as "blocker", "critical", "warning", "info", or "nitpick""#
        .to_string()
}

fn security_prompt() -> String {
    r#"You are a security-focused code reviewer analyzing a pull request diff. Focus exclusively on:
- Authentication bypass: missing or incorrect auth checks, token validation gaps
- Injection vulnerabilities: SQL injection, XSS, command injection, path traversal
- IDOR: insecure direct object references, missing authorization checks
- Secret exposure: hardcoded credentials, API keys, tokens in code or logs
- Logic flaws: race conditions, TOCTOU, improper error handling that leaks info

Rules:
- Only flag actionable security issues introduced by this change
- Provide file path and line range for each finding
- Assign a confidence score 0-1 for each finding
- Set agent_type to "security" for all findings
- Assign severity as "blocker", "critical", "warning", "info", or "nitpick"
- Ignore style, naming, or performance issues — those are handled by other reviewers"#
        .to_string()
}

fn architecture_prompt() -> String {
    r#"You are an architecture-focused code reviewer analyzing a pull request diff. Focus exclusively on:
- Boundary violations: layers importing from wrong layers, bypassing service boundaries
- Coupling drift: tight coupling between modules that should be independent
- Design inconsistencies: patterns used inconsistently, abstraction leaks
- API contract issues: breaking changes, inconsistent error handling patterns
- Dependency direction: circular dependencies, wrong dependency flow

Rules:
- Only flag actionable architecture issues introduced by this change
- Provide file path and line range for each finding
- Assign a confidence score 0-1 for each finding
- Set agent_type to "architecture" for all findings
- Assign severity as "blocker", "critical", "warning", "info", or "nitpick"
- Ignore security vulnerabilities, performance issues, or style nits — those are handled by other reviewers"#
        .to_string()
}

fn performance_prompt() -> String {
    r#"You are a performance-focused code reviewer analyzing a pull request diff. Focus exclusively on:
- N+1 query patterns: database queries inside loops
- Inefficient loops: unnecessary iterations, missing early exits, quadratic algorithms
- Memory pressure: unbounded allocations, large copies, missing streaming
- Needless I/O: redundant file reads, unnecessary network calls, missing caching
- Blocking operations: synchronous calls in async contexts, missing concurrency

Rules:
- Only flag actionable performance issues introduced by this change
- Provide file path and line range for each finding
- Assign a confidence score 0-1 for each finding
- Set agent_type to "performance" for all findings
- Assign severity as "blocker", "critical", "warning", "info", or "nitpick"
- Ignore security vulnerabilities, architecture issues, or style nits — those are handled by other reviewers"#
        .to_string()
}

pub fn build_review_input(focus: AgentFocus, diff: &str) -> ReviewInput {
    let system_prompt = match focus {
        AgentFocus::General => base_review_prompt(),
        AgentFocus::Security => security_prompt(),
        AgentFocus::Architecture => architecture_prompt(),
        AgentFocus::Performance => performance_prompt(),
    };

    ReviewInput {
        system_prompt,
        diff: diff.to_string(),
        output_schema: OUTPUT_SCHEMA.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_review_input_general() {
        let input = build_review_input(AgentFocus::General, "some diff");
        assert!(input.system_prompt.contains("Security"));
        assert!(input.system_prompt.contains("Architecture"));
        assert!(input.system_prompt.contains("Performance"));
        assert_eq!(input.diff, "some diff");
        assert!(input.output_schema.contains("findings"));
    }

    #[test]
    fn test_build_review_input_security() {
        let input = build_review_input(AgentFocus::Security, "diff");
        assert!(input.system_prompt.contains("security-focused"));
        assert!(input.system_prompt.contains("Injection"));
        assert!(!input.system_prompt.contains("N+1"));
    }

    #[test]
    fn test_build_review_input_architecture() {
        let input = build_review_input(AgentFocus::Architecture, "diff");
        assert!(input.system_prompt.contains("architecture-focused"));
        assert!(input.system_prompt.contains("Boundary violations"));
    }

    #[test]
    fn test_build_review_input_performance() {
        let input = build_review_input(AgentFocus::Performance, "diff");
        assert!(input.system_prompt.contains("performance-focused"));
        assert!(input.system_prompt.contains("N+1"));
    }

    #[test]
    fn test_agent_focus_as_str() {
        assert_eq!(AgentFocus::Security.as_str(), "security");
        assert_eq!(AgentFocus::Architecture.as_str(), "architecture");
        assert_eq!(AgentFocus::Performance.as_str(), "performance");
        assert_eq!(AgentFocus::General.as_str(), "general");
    }
}
