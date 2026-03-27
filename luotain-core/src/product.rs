//! Product tree — a product directory containing description, specs, and results.
//!
//! Layout:
//! ```text
//! products/myapp/
//!   product.md       ← product description
//!   _config.md       ← target, auth, environments
//!   specs/
//!     auth/login.md
//!   results/
//!     2026-03-27/
//!       auth/
//!         login.json
//! ```

use crate::result::SpecResult;
use crate::spec::{SpecError, SpecTree};
use std::path::{Path, PathBuf};
use thiserror::Error;

const PRODUCT_FILENAME: &str = "product.md";

#[derive(Debug, Error)]
pub enum ProductError {
    #[error("product directory not found: {0}")]
    NotFound(PathBuf),
    #[error("product.md not found in {0}")]
    NoProductFile(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("spec error: {0}")]
    Spec(#[from] SpecError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A product directory with description, specs, and results.
pub struct ProductTree {
    root: PathBuf,
}

impl ProductTree {
    /// Open a product directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ProductError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(ProductError::NotFound(root));
        }
        if !root.join(PRODUCT_FILENAME).is_file() {
            return Err(ProductError::NoProductFile(root));
        }
        Ok(Self { root })
    }

    /// Read product.md — the product description.
    pub fn read_product(&self) -> Result<String, ProductError> {
        Ok(std::fs::read_to_string(self.root.join(PRODUCT_FILENAME))?)
    }

    /// Open the specs/ subtree. Reuses SpecTree.
    pub fn specs(&self) -> Result<SpecTree, ProductError> {
        let specs_dir = self.root.join("specs");
        if !specs_dir.is_dir() {
            std::fs::create_dir_all(&specs_dir)?;
        }
        Ok(SpecTree::open(specs_dir)?)
    }

    /// Path to the results directory for a given date (YYYY-MM-DD).
    fn results_dir(&self, date: &str) -> PathBuf {
        self.root.join("results").join(date)
    }

    /// Write a SpecResult to results/YYYY-MM-DD/<spec_path>.json
    pub fn write_result(&self, date: &str, result: &SpecResult) -> Result<PathBuf, ProductError> {
        let spec_path = result.spec.trim_end_matches(".md");
        let result_path = self.results_dir(date).join(format!("{}.json", spec_path));

        if let Some(parent) = result_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(result)?;
        std::fs::write(&result_path, json)?;
        Ok(result_path)
    }

    /// Read all results for a given date. Returns vec of (spec_path, SpecResult).
    pub fn read_results(&self, date: &str) -> Result<Vec<SpecResult>, ProductError> {
        let dir = self.results_dir(date);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for entry in walkdir::WalkDir::new(&dir).sort_by_file_name() {
            let entry = entry.map_err(|e| ProductError::Io(e.into()))?;
            if entry.file_type().is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("json")
            {
                let content = std::fs::read_to_string(entry.path())?;
                let result: SpecResult = serde_json::from_str(&content)?;
                results.push(result);
            }
        }
        Ok(results)
    }

    /// Root path of the product directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::FeatureResult;
    use chrono::Utc;
    use std::fs;

    #[test]
    fn test_product_tree_roundtrip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();

        // Set up product structure
        fs::write(root.join("product.md"), "# MyApp\nA test product.").expect("write product.md");
        fs::create_dir_all(root.join("specs/auth")).expect("create specs dir");
        fs::write(root.join("specs/auth/login.md"), "# Login\n- Returns 200").expect("write spec");

        let tree = ProductTree::open(root).expect("open product");

        // Read product
        let product = tree.read_product().expect("read product");
        assert!(product.contains("MyApp"));

        // List specs
        let specs = tree.specs().expect("open specs");
        let spec_list = specs.list_specs().expect("list specs");
        assert_eq!(spec_list, vec!["auth/login.md"]);

        // Write result
        let result = SpecResult {
            spec: "auth/login.md".into(),
            timestamp: Utc::now(),
            verdict: "pass".into(),
            features: vec![FeatureResult {
                description: "Returns 200".into(),
                verdict: "pass".into(),
                why: None,
            }],
            probes: vec![],
            mode: None,
        };
        let path = tree.write_result("2026-03-27", &result).expect("write result");
        assert!(path.exists());

        // Read results
        let results = tree.read_results("2026-03-27").expect("read results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].spec, "auth/login.md");
        assert_eq!(results[0].verdict, "pass");
    }
}
