use super::RepoConfig;

/// Merge two RepoConfigs. `overlay` values take precedence over `base`.
/// Arrays (like `lanes`) replace entirely (not append).
/// Objects/scalars from overlay override base when present.
pub fn deep_merge_configs(base: RepoConfig, overlay: RepoConfig) -> RepoConfig {
    RepoConfig {
        extends: overlay.extends.or(base.extends),
        lanes: overlay.lanes.or(base.lanes),
        max_findings: overlay.max_findings.or(base.max_findings),
        similarity_threshold: overlay.similarity_threshold.or(base.similarity_threshold),
        drop_nitpicks: overlay.drop_nitpicks.or(base.drop_nitpicks),
        min_confidence: overlay.min_confidence.or(base.min_confidence),
        lane_timeout_secs: overlay.lane_timeout_secs.or(base.lane_timeout_secs),
        preferred_provider: overlay.preferred_provider.or(base.preferred_provider),
        custom_agents: overlay.custom_agents.or(base.custom_agents),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_replaces_arrays() {
        let base = RepoConfig {
            lanes: Some(vec!["security".into(), "architecture".into()]),
            max_findings: Some(10),
            ..Default::default()
        };
        let overlay = RepoConfig {
            lanes: Some(vec!["performance".into()]),
            ..Default::default()
        };
        let merged = deep_merge_configs(base, overlay);
        assert_eq!(merged.lanes, Some(vec!["performance".into()]));
        // base scalar preserved when overlay is None
        assert_eq!(merged.max_findings, Some(10));
    }

    #[test]
    fn test_overlay_fills_missing_scalars() {
        let base = RepoConfig {
            max_findings: Some(5),
            similarity_threshold: Some(0.8),
            drop_nitpicks: Some(true),
            ..Default::default()
        };
        let overlay = RepoConfig {
            min_confidence: Some(0.6),
            ..Default::default()
        };
        let merged = deep_merge_configs(base, overlay);
        assert_eq!(merged.max_findings, Some(5));
        assert_eq!(merged.similarity_threshold, Some(0.8));
        assert_eq!(merged.drop_nitpicks, Some(true));
        assert_eq!(merged.min_confidence, Some(0.6));
    }

    #[test]
    fn test_both_none_results_in_none() {
        let base = RepoConfig::default();
        let overlay = RepoConfig::default();
        let merged = deep_merge_configs(base, overlay);
        assert!(merged.lanes.is_none());
        assert!(merged.max_findings.is_none());
        assert!(merged.similarity_threshold.is_none());
        assert!(merged.drop_nitpicks.is_none());
        assert!(merged.min_confidence.is_none());
        assert!(merged.lane_timeout_secs.is_none());
        assert!(merged.preferred_provider.is_none());
        assert!(merged.extends.is_none());
        assert!(merged.custom_agents.is_none());
    }

    #[test]
    fn test_overlay_scalar_overrides_base() {
        let base = RepoConfig {
            max_findings: Some(10),
            preferred_provider: Some("claude".into()),
            ..Default::default()
        };
        let overlay = RepoConfig {
            max_findings: Some(3),
            preferred_provider: Some("codex".into()),
            ..Default::default()
        };
        let merged = deep_merge_configs(base, overlay);
        assert_eq!(merged.max_findings, Some(3));
        assert_eq!(merged.preferred_provider, Some("codex".into()));
    }
}
