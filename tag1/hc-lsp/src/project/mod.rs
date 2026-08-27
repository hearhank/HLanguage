//! LSP 项目上下文：项目根目录与 build.zon 解析
//!
//! 定义：结构体：ProjectContext

use std::path::PathBuf;
use tower_lsp::lsp_types::Url;

/// Represents the project context
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// The root URI of the project
    pub root_uri: Option<Url>,
    /// The root path of the project (if available)
    pub root_path: Option<PathBuf>,
    /// The project name (from build.zon)
    pub name: Option<String>,
    /// The project version (from build.zon)
    pub version: Option<String>,
}

impl ProjectContext {
    /// Create a new project context
    pub fn new() -> Self {
        Self {
            root_uri: None,
            root_path: None,
            name: None,
            version: None,
        }
    }

    /// Set the root URI
    pub fn set_root_uri(&mut self, uri: Url) {
        self.root_uri = Some(uri.clone());
        // Try to convert URI to path
        if let Ok(path) = uri.to_file_path() {
            self.root_path = Some(path);
        }
    }

    /// Set the root path
    pub fn set_root_path(&mut self, path: PathBuf) {
        self.root_path = Some(path.clone());
        // Try to convert path to URI
        if let Ok(uri) = Url::from_file_path(&path) {
            self.root_uri = Some(uri);
        }
    }

    /// Get the root URI
    pub fn root_uri(&self) -> Option<&Url> {
        self.root_uri.as_ref()
    }

    /// Get the root path
    pub fn root_path(&self) -> Option<&PathBuf> {
        self.root_path.as_ref()
    }

    /// Check if the project has a root
    pub fn has_root(&self) -> bool {
        self.root_uri.is_some() || self.root_path.is_some()
    }

    /// Find the build.zon file in the project root
    pub fn find_build_zon(&self) -> Option<PathBuf> {
        self.root_path.as_ref().map(|root| root.join("build.zon"))
    }

    /// Parse build.zon and update project info
    /// Note: This is a placeholder. The actual implementation will reuse hc-tools/src/buildzon.rs
    pub fn parse_build_zon(&mut self) -> Result<(), String> {
        // TODO: Implement build.zon parsing
        // For now, we just set placeholder values
        self.name = Some("unknown".to_string());
        self.version = Some("0.1.0".to_string());
        Ok(())
    }
}

impl Default for ProjectContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
