use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Result of discovering a local clone of a GitHub repo.
#[derive(Debug, Clone)]
pub struct FoundRepo {
    pub name: String,
    pub path: PathBuf,
    pub is_primary: bool, // found under configured repo_dir (pass 1)
}

/// Pass 1: Find repos under the configured repo_dir (maxdepth 3).
pub fn find_in_primary(repo_dir: &Path, names: &[String]) -> Vec<FoundRepo> {
    let names_lower: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let mut found = Vec::new();

    scan_for_git_repos(repo_dir, 3, &mut |path| {
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if names_lower.contains(&dir_name.to_lowercase()) {
            found.push(FoundRepo {
                name: dir_name.to_string(),
                path: path.to_path_buf(),
                is_primary: true,
            });
        }
    });

    found
}

/// Pass 2: Find repos elsewhere under $HOME (maxdepth 4), excluding
/// Library, .cache, node_modules, .Trash, and the primary repo_dir itself.
pub fn find_elsewhere(
    home: &Path,
    repo_dir: &Path,
    names: &[String],
    already_found: &HashSet<String>,
) -> Vec<FoundRepo> {
    let remaining: HashSet<String> = names
        .iter()
        .filter(|n| !already_found.contains(&n.to_lowercase()))
        .map(|n| n.to_lowercase())
        .collect();

    if remaining.is_empty() {
        return Vec::new();
    }

    let mut found = Vec::new();

    scan_for_git_repos(home, 4, &mut |path| {
        // Skip excluded directories
        if is_excluded(path, repo_dir) {
            return;
        }

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if remaining.contains(&dir_name.to_lowercase()) {
            found.push(FoundRepo {
                name: dir_name.to_string(),
                path: path.to_path_buf(),
                is_primary: false,
            });
        }
    });

    found
}

/// Pass 3: Find all dirty repos under repo_dir (maxdepth 4),
/// regardless of whether they have GitHub activity.
/// Returns paths — the caller checks git status to filter for dirty ones.
pub fn find_all_local(repo_dir: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    scan_for_git_repos(repo_dir, 4, &mut |path| {
        repos.push(path.to_path_buf());
    });
    repos
}

/// Walk directories up to `max_depth` looking for `.git` dirs.
/// Calls `callback` with the parent of each `.git` found.
fn scan_for_git_repos(root: &Path, max_depth: usize, callback: &mut dyn FnMut(&Path)) {
    scan_recursive(root, 0, max_depth, callback);
}

fn scan_recursive(dir: &Path, depth: usize, max_depth: usize, callback: &mut dyn FnMut(&Path)) {
    if depth > max_depth {
        return;
    }

    let git_dir = dir.join(".git");
    if git_dir.is_dir() {
        callback(dir);
        return; // don't descend into git repos
    }

    if depth == max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Skip hidden dirs (except the root) and common large dirs
            if name_str.starts_with('.') || name_str == "node_modules" {
                continue;
            }
            scan_recursive(&path, depth + 1, max_depth, callback);
        }
    }
}

fn is_excluded(path: &Path, repo_dir: &Path) -> bool {
    let s = path.to_string_lossy();
    path.starts_with(repo_dir)
        || s.contains("/Library/")
        || s.contains("/.Trash/")
        || s.contains("/.cache/")
        || s.contains("/node_modules/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_repos(base: &Path) -> Vec<String> {
        // Create fake repos
        for name in ["alpha", "beta", "gamma"] {
            let repo = base.join(name);
            fs::create_dir_all(repo.join(".git")).unwrap();
        }
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    }

    #[test]
    fn find_in_primary_finds_matching_repos() {
        let dir = std::env::temp_dir().join("fleetz_test_primary");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let names = setup_test_repos(&dir);

        let found = find_in_primary(&dir, &names);
        let found_names: HashSet<_> = found.iter().map(|f| f.name.clone()).collect();
        assert!(found_names.contains("alpha"));
        assert!(found_names.contains("beta"));
        assert!(found_names.contains("gamma"));
        assert!(found.iter().all(|f| f.is_primary));

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn find_in_primary_is_case_insensitive() {
        let dir = std::env::temp_dir().join("fleetz_test_case");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("MyRepo").join(".git")).unwrap();

        let found = find_in_primary(&dir, &["myrepo".to_string()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "MyRepo");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn find_in_primary_ignores_non_matching() {
        let dir = std::env::temp_dir().join("fleetz_test_nomatch");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("other").join(".git")).unwrap();

        let found = find_in_primary(&dir, &["nothere".to_string()]);
        assert!(found.is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn find_all_local_returns_all_git_dirs() {
        let dir = std::env::temp_dir().join("fleetz_test_all");
        let _ = fs::remove_dir_all(&dir);
        setup_test_repos(&dir);

        let all = find_all_local(&dir);
        assert_eq!(all.len(), 3);

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn scan_respects_max_depth() {
        let dir = std::env::temp_dir().join("fleetz_test_depth");
        let _ = fs::remove_dir_all(&dir);
        // Create a repo at depth 5 — should not be found at maxdepth 3
        let deep = dir.join("a").join("b").join("c").join("d").join("deep");
        fs::create_dir_all(deep.join(".git")).unwrap();

        let found = find_in_primary(&dir, &["deep".to_string()]);
        assert!(found.is_empty());

        fs::remove_dir_all(dir).ok();
    }
}
