use std::path::Path;
use std::time::SystemTime;
use std::{io::Read, path::Component, time::Duration};

use sha2::{Digest, Sha256};

use super::RepoConfig;
use crate::config::merge::deep_merge_configs;

const MAX_EXTENDS_DEPTH: usize = 5;
const CACHE_TTL_SECS: u64 = 3600; // 1 hour

/// Resolve the `extends` field by loading the parent config.
/// Supports local file paths (relative to workspace) and HTTPS URLs (cached with 1hr TTL).
/// Max depth: 5 to prevent circular references.
/// Returns None if extends is None or if the parent cannot be loaded (with a warning log).
pub fn resolve_extends(
    extends: &str,
    workspace_path: &Path,
    cache_dir: &Path,
    depth: usize,
) -> Option<RepoConfig> {
    if depth >= MAX_EXTENDS_DEPTH {
        tracing::warn!(
            "Config extends depth limit ({}) reached, ignoring further extends",
            MAX_EXTENDS_DEPTH
        );
        return None;
    }

    let content = if extends.starts_with("http://") {
        tracing::warn!("Config extends must use https:// URLs (got http://)");
        return None;
    } else if extends.starts_with("https://") {
        load_from_url(extends, cache_dir)?
    } else {
        load_from_file(extends, workspace_path)?
    };

    let mut parent: RepoConfig = match serde_yml::from_str(&content) {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("Failed to parse extended config '{}': {}", extends, e);
            return None;
        }
    };

    // Recursively resolve if parent also has extends
    if let Some(ref parent_extends) = parent.extends.clone() {
        let grandparent_workspace = if extends.starts_with("https://") {
            workspace_path.to_path_buf()
        } else {
            // Resolve relative to the parent config's directory
            let parent_path = workspace_path.join(extends);
            parent_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| workspace_path.to_path_buf())
        };
        if let Some(grandparent) =
            resolve_extends(parent_extends, &grandparent_workspace, cache_dir, depth + 1)
        {
            parent = deep_merge_configs(grandparent, parent);
        }
    }

    // Clear the extends field from the resolved parent since it's been resolved
    parent.extends = None;
    Some(parent)
}

fn load_from_file(relative_path: &str, workspace_path: &Path) -> Option<String> {
    if !is_safe_relative_path(relative_path) {
        tracing::warn!(
            "Refusing to load extended config with unsafe path '{}'",
            relative_path
        );
        return None;
    }

    let path = workspace_path.join(relative_path);
    match std::fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::warn!(
                "Failed to read extended config file '{}': {}",
                path.display(),
                e
            );
            None
        }
    }
}

fn load_from_url(url: &str, cache_dir: &Path) -> Option<String> {
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let cache_path = cache_dir.join(format!("{}.yml", hash));

    // Check cache freshness
    if let Ok(metadata) = std::fs::metadata(&cache_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                if elapsed.as_secs() < CACHE_TTL_SECS {
                    if let Ok(content) = std::fs::read_to_string(&cache_path) {
                        return Some(content);
                    }
                }
            }
        }
    }

    // Fetch from URL with conservative timeouts and a small size cap.
    // This function is called during config resolution; we avoid unbounded hangs and downloads.
    const MAX_BYTES: u64 = 512 * 1024; // 512KiB should be plenty for YAML presets

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to build reqwest client: {}", e);
            return std::fs::read_to_string(&cache_path).ok();
        }
    };

    match client.get(url).send() {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::warn!(
                    "Failed to fetch extended config from '{}': HTTP {}",
                    url,
                    resp.status()
                );
                // Fall back to stale cache if available
                return std::fs::read_to_string(&cache_path).ok();
            }
            let resp = resp;
            if let Some(len) = resp.content_length() {
                if len > MAX_BYTES {
                    tracing::warn!(
                        "Extended config '{}' too large ({} bytes), refusing to download",
                        url,
                        len
                    );
                    return std::fs::read_to_string(&cache_path).ok();
                }
            }

            let mut body = String::new();
            match resp.take(MAX_BYTES + 1).read_to_string(&mut body) {
                Ok(read) if (read as u64) <= MAX_BYTES => {
                    // Cache the response
                    if let Err(e) = std::fs::create_dir_all(cache_dir) {
                        tracing::warn!("Failed to create preset cache dir: {}", e);
                    }
                    if let Err(e) = std::fs::write(&cache_path, &body) {
                        tracing::warn!("Failed to write preset cache: {}", e);
                    }
                    Some(body)
                }
                Ok(_) => {
                    tracing::warn!("Extended config '{}' exceeded size limit", url);
                    std::fs::read_to_string(&cache_path).ok()
                }
                Err(e) => {
                    tracing::warn!("Failed to read response body from '{}': {}", url, e);
                    std::fs::read_to_string(&cache_path).ok()
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to fetch extended config from '{}': {}", url, e);
            // Fall back to stale cache if available
            std::fs::read_to_string(&cache_path).ok()
        }
    }
}

fn is_safe_relative_path(p: &str) -> bool {
    let path = std::path::Path::new(p);
    if path.is_absolute() {
        return false;
    }
    for c in path.components() {
        match c {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_with_extends() {
        let yaml = "extends: \"./base.yml\"\nmax_findings: 3\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.extends, Some("./base.yml".into()));
        assert_eq!(config.max_findings, Some(3));
    }

    #[test]
    fn test_missing_extends_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");
        let result = resolve_extends("nonexistent.yml", dir.path(), &cache_dir, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_local_extends_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        // Write the base config
        std::fs::write(
            dir.path().join("base.yml"),
            "max_findings: 10\ndrop_nitpicks: true\n",
        )
        .unwrap();

        let result = resolve_extends("base.yml", dir.path(), &cache_dir, 0);
        let parent = result.expect("should resolve local extends");
        assert_eq!(parent.max_findings, Some(10));
        assert_eq!(parent.drop_nitpicks, Some(true));
    }

    #[test]
    fn test_depth_limit_prevents_infinite_recursion() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        // Write a config that extends itself (circular)
        std::fs::write(
            dir.path().join("loop.yml"),
            "extends: \"loop.yml\"\nmax_findings: 1\n",
        )
        .unwrap();

        // Should not panic, should return Some at depth 0 but the inner resolve at depth 4
        // will hit the limit at depth 5
        let result = resolve_extends("loop.yml", dir.path(), &cache_dir, 0);
        assert!(result.is_some()); // The first level resolves, just stops recursing
        assert_eq!(result.unwrap().max_findings, Some(1));
    }

    #[test]
    fn test_depth_limit_at_max_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        let result = resolve_extends("anything.yml", dir.path(), &cache_dir, MAX_EXTENDS_DEPTH);
        assert!(result.is_none());
    }

    #[test]
    fn test_chained_extends() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        // grandparent.yml -> no extends
        std::fs::write(
            dir.path().join("grandparent.yml"),
            "max_findings: 20\nsimilarity_threshold: 0.5\n",
        )
        .unwrap();

        // parent.yml -> extends grandparent.yml, overrides max_findings
        std::fs::write(
            dir.path().join("parent.yml"),
            "extends: \"grandparent.yml\"\nmax_findings: 10\n",
        )
        .unwrap();

        let result = resolve_extends("parent.yml", dir.path(), &cache_dir, 0);
        let config = result.expect("should resolve chained extends");
        // max_findings from parent overrides grandparent
        assert_eq!(config.max_findings, Some(10));
        // similarity_threshold inherited from grandparent
        assert_eq!(config.similarity_threshold, Some(0.5));
    }

    #[test]
    fn test_malformed_parent_yaml_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        std::fs::write(dir.path().join("bad.yml"), "{{invalid yaml").unwrap();

        let result = resolve_extends("bad.yml", dir.path(), &cache_dir, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_yaml_with_unknown_fields_tolerates_them() {
        let yaml = "extends: \"./base.yml\"\nunknown_future_field: 42\nmax_findings: 3\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.extends, Some("./base.yml".into()));
        assert_eq!(config.max_findings, Some(3));
    }

    #[test]
    fn test_local_extends_rejects_parent_dir_escape() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        // Attempt to escape the workspace root.
        let result = resolve_extends("../secrets.yml", dir.path(), &cache_dir, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_http_extends_rejected_requires_https() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("cache");

        let result = resolve_extends("http://example.com/preset.yml", dir.path(), &cache_dir, 0);
        assert!(result.is_none());
    }
}
