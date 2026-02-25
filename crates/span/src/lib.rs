//! `kyokara-span` — Source locations and file identity.
//!
//! Defines [`FileId`], [`Span`], [`FileMap`], and re-exports `text-size`
//! types ([`TextRange`], [`TextSize`]) used throughout the compiler to
//! track where things come from.

use std::collections::HashMap;
use std::path::PathBuf;

pub use text_size::{TextRange, TextSize};

/// Opaque handle identifying a source file within a compilation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

impl FileId {
    pub fn new(n: u32) -> Self {
        Self(n)
    }
}

/// A source span: a file plus a range within that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub range: TextRange,
}

/// Bidirectional mapping between [`FileId`]s and filesystem paths.
#[derive(Debug, Default)]
pub struct FileMap {
    path_to_id: HashMap<PathBuf, FileId>,
    id_to_path: HashMap<FileId, PathBuf>,
    next_id: u32,
}

impl FileMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a file path and return its ID. If already registered,
    /// returns the existing ID.
    pub fn insert(&mut self, path: PathBuf) -> FileId {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }
        let id = FileId::new(self.next_id);
        self.next_id += 1;
        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path);
        id
    }

    /// Look up the path for a file ID.
    pub fn path(&self, id: FileId) -> Option<&PathBuf> {
        self.id_to_path.get(&id)
    }

    /// Look up the file ID for a path.
    pub fn id(&self, path: &PathBuf) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }
}
