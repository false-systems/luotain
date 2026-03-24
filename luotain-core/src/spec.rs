use serde::Serialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("spec root not found: {0}")]
    RootNotFound(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("walk error: {0}")]
    Walk(#[from] walkdir::Error),
}

/// A node in the spec tree — either a directory or a spec file.
#[derive(Debug, Clone, Serialize)]
pub struct SpecNode {
    /// Relative path from spec root
    pub path: String,
    /// Display name
    pub name: String,
    pub kind: SpecNodeKind,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SpecNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecNodeKind {
    Directory,
    Spec,
}

/// The spec tree — a directory of markdown files describing expected behavior.
///
/// The agent reads specs to understand what to test. Luotain walks the tree
/// and presents the structure. The directory hierarchy mirrors the software
/// being tested.
pub struct SpecTree {
    root: PathBuf,
}

impl SpecTree {
    /// Open a spec tree rooted at the given directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SpecError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(SpecError::RootNotFound(root));
        }
        Ok(Self { root })
    }

    /// Walk the spec tree and return the full tree structure.
    pub fn walk(&self) -> Result<SpecNode, SpecError> {
        self.build_node(&self.root, "")
    }

    /// Read a spec file by relative path.
    pub fn read_spec(&self, rel_path: &str) -> Result<String, SpecError> {
        let full_path = self.root.join(rel_path);
        Ok(std::fs::read_to_string(full_path)?)
    }

    /// List all spec file paths (relative), sorted.
    pub fn list_specs(&self) -> Result<Vec<String>, SpecError> {
        let mut specs = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root).sort_by_file_name() {
            let entry = entry?;
            if entry.file_type().is_file() {
                let ext = entry.path().extension().and_then(|e| e.to_str());
                if ext == Some("md") {
                    if let Ok(rel) = entry.path().strip_prefix(&self.root) {
                        specs.push(rel.to_string_lossy().to_string());
                    }
                }
            }
        }
        Ok(specs)
    }

    fn build_node(&self, path: &Path, rel: &str) -> Result<SpecNode, SpecError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if path.is_file() {
            let display_name = path
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(name);
            return Ok(SpecNode {
                path: rel.to_string(),
                name: display_name,
                kind: SpecNodeKind::Spec,
                children: vec![],
            });
        }

        let mut children = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let child_path = entry.path();
            let child_name = child_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let child_rel = if rel.is_empty() {
                child_name.clone()
            } else {
                format!("{}/{}", rel, child_name)
            };

            if child_path.is_dir() {
                children.push(self.build_node(&child_path, &child_rel)?);
            } else if child_path.extension().and_then(|e| e.to_str()) == Some("md") {
                children.push(self.build_node(&child_path, &child_rel)?);
            }
        }

        Ok(SpecNode {
            path: rel.to_string(),
            name: if rel.is_empty() {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "root".to_string())
            } else {
                name
            },
            kind: SpecNodeKind::Directory,
            children,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_walk_spec_tree() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let auth = dir.path().join("auth");
        fs::create_dir(&auth).expect("create auth dir");
        fs::write(auth.join("login.md"), "# Login").expect("write login.md");
        fs::write(auth.join("signup.md"), "# Signup").expect("write signup.md");
        fs::write(dir.path().join("readme.md"), "# Root").expect("write readme.md");

        let tree = SpecTree::open(dir.path()).expect("open tree");
        let root = tree.walk().expect("walk tree");

        assert_eq!(root.kind, SpecNodeKind::Directory);
        assert_eq!(root.children.len(), 2); // auth dir + readme.md

        let specs = tree.list_specs().expect("list specs");
        assert_eq!(specs.len(), 3);
    }

    #[test]
    fn test_read_spec() {
        let dir = tempfile::tempdir().expect("create temp dir");
        fs::write(dir.path().join("test.md"), "# Test\nHello").expect("write");

        let tree = SpecTree::open(dir.path()).expect("open tree");
        let content = tree.read_spec("test.md").expect("read spec");
        assert_eq!(content, "# Test\nHello");
    }
}
