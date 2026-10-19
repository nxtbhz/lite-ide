use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Crash recovery journal: records unsaved buffer snapshots to disk.
/// If a crash (e.g. kill -9) occurs, the file's unsaved edits can be fully restored.
pub struct Journal;

impl Journal {
    pub fn journal_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("LUME_JOURNAL_DIR") {
            PathBuf::from(dir)
        } else if let Ok(cache) = std::env::var("XDG_CACHE_HOME") {
            PathBuf::from(cache).join("lume/journal")
        } else {
            std::env::temp_dir().join("lume_journal")
        }
    }

    fn journal_path(file_path: &Path) -> PathBuf {
        let dir = Self::journal_dir();
        // Generate stable filename based on path hash
        let hash = path_hash(file_path);
        let name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");
        dir.join(format!("{:016x}_{}.journal", hash, name))
    }

    pub fn record(file_path: &Path, content: &str) -> io::Result<()> {
        let path = Self::journal_path(file_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Write content atomically or directly
        fs::write(&path, content)
    }

    pub fn has_journal(file_path: &Path) -> bool {
        let path = Self::journal_path(file_path);
        path.exists()
    }

    pub fn recover(file_path: &Path) -> io::Result<Option<String>> {
        let path = Self::journal_path(file_path);
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }

    pub fn remove(file_path: &Path) -> io::Result<()> {
        let path = Self::journal_path(file_path);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn path_hash(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_record_recover_remove() {
        let tmp = std::env::temp_dir().join(format!("lume_test_j_{}", std::process::id()));
        std::env::set_var("LUME_JOURNAL_DIR", &tmp);

        let target = Path::new("/tmp/test_code.c");
        assert!(!Journal::has_journal(target));

        Journal::record(target, "int main() { return 42; }").unwrap();
        assert!(Journal::has_journal(target));

        let recovered = Journal::recover(target).unwrap().unwrap();
        assert_eq!(recovered, "int main() { return 42; }");

        Journal::remove(target).unwrap();
        assert!(!Journal::has_journal(target));

        let _ = fs::remove_dir_all(&tmp);
    }
}
