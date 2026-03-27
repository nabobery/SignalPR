use std::path::{Path, PathBuf};

use crate::errors::AppError;

use super::patch::search_replace_to_unified_diff;

fn is_safe_relative_path(p: &str) -> bool {
    let path = std::path::Path::new(p);
    if path.is_absolute() {
        return false;
    }
    for c in path.components() {
        match c {
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return false,
            _ => {}
        }
    }
    true
}

fn resolve_workspace_file(workspace_path: &Path, file_path: &str) -> Result<PathBuf, AppError> {
    if !is_safe_relative_path(file_path) {
        return Err(AppError::InvalidInput(format!(
            "Refusing to access unsafe file path: {}",
            file_path
        )));
    }

    let ws = workspace_path.canonicalize().map_err(AppError::Io)?;
    let candidate = workspace_path.join(file_path);
    let full_path = candidate.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound(format!("File not found: {}", candidate.display()))
        } else {
            AppError::Io(e)
        }
    })?;

    if !full_path.starts_with(&ws) {
        return Err(AppError::InvalidInput(format!(
            "Refusing to access file outside workspace: {}",
            file_path
        )));
    }

    Ok(full_path)
}

/// Preview a fix by returning the unified diff without modifying the file.
pub fn preview_fix(
    workspace_path: &Path,
    file_path: &str,
    search: &str,
    replace: &str,
) -> Result<String, AppError> {
    let full_path = resolve_workspace_file(workspace_path, file_path)?;
    let content = std::fs::read_to_string(&full_path).map_err(AppError::Io)?;

    search_replace_to_unified_diff(file_path, &content, search, replace)
        .ok_or_else(|| AppError::InvalidInput(format!("Search text not found in {}", file_path)))
}

/// Apply a fix by performing search/replace on the actual file.
pub fn apply_fix(
    workspace_path: &Path,
    file_path: &str,
    search: &str,
    replace: &str,
) -> Result<(), AppError> {
    let full_path = resolve_workspace_file(workspace_path, file_path)?;
    let content = std::fs::read_to_string(&full_path).map_err(AppError::Io)?;

    if !content.contains(search) {
        return Err(AppError::InvalidInput(format!(
            "Search text not found in {}",
            file_path
        )));
    }

    let updated = content.replacen(search, replace, 1);
    std::fs::write(&full_path, updated)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    fn setup_workspace(file_path: &str, content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let full = dir.path().join(file_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, content).unwrap();
        dir
    }

    #[test]
    fn test_preview_fix_returns_diff() {
        let dir = setup_workspace("src/main.rs", "fn main() {\n    println!(\"hello\");\n}\n");
        let result = preview_fix(
            dir.path(),
            "src/main.rs",
            "    println!(\"hello\");",
            "    println!(\"world\");",
        );
        assert!(result.is_ok());
        let diff = result.unwrap();
        assert!(diff.contains("--- a/src/main.rs"));
        assert!(diff.contains("+++ b/src/main.rs"));
        assert!(diff.contains("-    println!(\"hello\");"));
        assert!(diff.contains("+    println!(\"world\");"));
    }

    #[test]
    fn test_preview_fix_file_not_found() {
        let dir = TempDir::new().unwrap();
        let result = preview_fix(dir.path(), "nonexistent.rs", "search", "replace");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn test_preview_fix_rejects_parent_dir_escape() {
        let dir = TempDir::new().unwrap();
        let result = preview_fix(dir.path(), "../outside.rs", "a", "b");
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn test_apply_fix_rejects_absolute_path() {
        let dir = TempDir::new().unwrap();
        let result = apply_fix(dir.path(), "/etc/passwd", "a", "b");
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[cfg(unix)]
    #[test]
    fn test_preview_fix_rejects_symlink_escape() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("outside.txt");
        fs::write(&outside_file, "secret").unwrap();

        let link = dir.path().join("link.txt");
        symlink(&outside_file, &link).unwrap();

        let result = preview_fix(dir.path(), "link.txt", "secret", "public");
        assert!(matches!(result, Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn test_preview_fix_search_not_found() {
        let dir = setup_workspace("file.rs", "let x = 1;");
        let result = preview_fix(dir.path(), "file.rs", "let y = 2;", "let y = 3;");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[test]
    fn test_apply_fix_modifies_file() {
        let dir = setup_workspace(
            "src/lib.rs",
            "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        );
        let result = apply_fix(dir.path(), "src/lib.rs", "a + b", "a.wrapping_add(b)");
        assert!(result.is_ok());

        let content = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("a.wrapping_add(b)"));
        assert!(!content.contains("a + b"));
    }

    #[test]
    fn test_apply_fix_file_not_found() {
        let dir = TempDir::new().unwrap();
        let result = apply_fix(dir.path(), "missing.rs", "a", "b");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn test_apply_fix_search_not_found() {
        let dir = setup_workspace("file.rs", "let x = 1;");
        let result = apply_fix(dir.path(), "file.rs", "let y = 2;", "let y = 3;");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::InvalidInput(_)));
    }

    #[test]
    fn test_apply_fix_only_replaces_first_occurrence() {
        let dir = setup_workspace("file.rs", "foo\nfoo\nfoo\n");
        let result = apply_fix(dir.path(), "file.rs", "foo", "bar");
        assert!(result.is_ok());

        let content = fs::read_to_string(dir.path().join("file.rs")).unwrap();
        assert_eq!(content, "bar\nfoo\nfoo\n");
    }

    #[test]
    fn test_preview_does_not_modify_file() {
        let dir = setup_workspace("file.rs", "let x = 1;");
        let _ = preview_fix(dir.path(), "file.rs", "let x = 1;", "let x = 2;");
        let content = fs::read_to_string(dir.path().join("file.rs")).unwrap();
        assert_eq!(content, "let x = 1;");
    }
}
