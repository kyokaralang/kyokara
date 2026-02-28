//! Corpus persistence — save and load failing test cases.
//!
//! Corpus entries are stored as JSON files under `.kyokara/test-corpus/<fn_name>/`.

use std::path::{Path, PathBuf};

use crate::choice::ChoiceSequence;

/// A single corpus entry (a saved failing test case).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CorpusEntry {
    pub function: String,
    pub choices: Vec<u64>,
    pub maxima: Vec<u64>,
    pub error: String,
    pub args_display: Vec<String>,
}

impl CorpusEntry {
    pub fn into_choice_sequence(self) -> ChoiceSequence {
        ChoiceSequence::new(self.choices, self.maxima)
    }
}

/// Get the corpus directory for a given base path and function name.
fn corpus_dir(base: &Path, fn_name: &str) -> PathBuf {
    base.join(".kyokara").join("test-corpus").join(fn_name)
}

/// Save a corpus entry to disk.
pub fn save_entry(base: &Path, entry: &CorpusEntry) -> Result<PathBuf, std::io::Error> {
    let dir = corpus_dir(base, &entry.function);
    std::fs::create_dir_all(&dir)?;

    // Find the next available index.
    let mut idx = 1u32;
    loop {
        let path = dir.join(format!("{idx:04}.json"));
        if !path.exists() {
            let json = serde_json::to_string_pretty(entry).map_err(std::io::Error::other)?;
            std::fs::write(&path, json)?;
            return Ok(path);
        }
        idx += 1;
        if idx > 9999 {
            return Err(std::io::Error::other("corpus directory full"));
        }
    }
}

/// Load all corpus entries for a given function.
pub fn load_entries(base: &Path, fn_name: &str) -> Vec<CorpusEntry> {
    let dir = corpus_dir(base, fn_name);
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && let Ok(contents) = std::fs::read_to_string(&path)
            && let Ok(corpus_entry) = serde_json::from_str::<CorpusEntry>(&contents)
        {
            entries.push(corpus_entry);
        }
    }
    entries
}

/// Check if any corpus entries exist for any function.
pub fn has_any_corpus(base: &Path) -> bool {
    let corpus_root = base.join(".kyokara").join("test-corpus");
    let Ok(read_dir) = std::fs::read_dir(&corpus_root) else {
        return false;
    };
    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        if entry.path().is_dir() {
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for f in inner {
                let Ok(f) = f else { continue };
                if f.path().extension().is_some_and(|e| e == "json") {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn round_trip_corpus_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entry = CorpusEntry {
            function: "divide".to_string(),
            choices: vec![42, 0],
            maxima: vec![256, 256],
            error: "postcondition failed: divide".to_string(),
            args_display: vec!["42".to_string(), "0".to_string()],
        };

        let path = save_entry(dir.path(), &entry).unwrap();
        assert!(path.exists());

        let loaded = load_entries(dir.path(), "divide");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].function, "divide");
        assert_eq!(loaded[0].choices, vec![42, 0]);
    }

    #[test]
    fn load_entries_returns_empty_for_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_entries(dir.path(), "nonexistent");
        assert!(loaded.is_empty());
    }

    #[test]
    fn has_any_corpus_detects_entries() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_any_corpus(dir.path()));

        let entry = CorpusEntry {
            function: "foo".to_string(),
            choices: vec![1],
            maxima: vec![10],
            error: "test".to_string(),
            args_display: vec!["1".to_string()],
        };
        save_entry(dir.path(), &entry).unwrap();
        assert!(has_any_corpus(dir.path()));
    }
}
