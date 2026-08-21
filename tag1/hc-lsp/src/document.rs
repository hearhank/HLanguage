use std::collections::HashMap;
use tower_lsp::lsp_types::*;

/// Represents a single document managed by the LSP server
#[derive(Debug, Clone)]
pub struct Document {
    /// The URI of the document
    pub uri: Url,
    /// The content of the document
    pub content: String,
    /// The version of the document (incremented on each change)
    pub version: i32,
    /// The language ID of the document
    pub language_id: String,
}

impl Document {
    /// Create a new document
    pub fn new(uri: Url, content: String, version: i32, language_id: String) -> Self {
        Self {
            uri,
            content,
            version,
            language_id,
        }
    }

    /// Update the document content
    pub fn update(&mut self, content: String, version: i32) {
        self.content = content;
        self.version = version;
    }
}

/// Manages all open documents in the workspace
#[derive(Debug, Default)]
pub struct DocumentManager {
    /// Map from document URI to Document
    documents: HashMap<Url, Document>,
}

impl DocumentManager {
    /// Create a new document manager
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    /// Open a document
    pub fn open(&mut self, uri: Url, content: String, version: i32, language_id: String) {
        let document = Document::new(uri.clone(), content, version, language_id);
        self.documents.insert(uri, document);
    }

    /// Close a document
    pub fn close(&mut self, uri: &Url) -> Option<Document> {
        self.documents.remove(uri)
    }

    /// Update a document
    pub fn update(&mut self, uri: &Url, content: String, version: i32) -> bool {
        if let Some(document) = self.documents.get_mut(uri) {
            document.update(content, version);
            true
        } else {
            false
        }
    }

    /// Get a document by URI
    pub fn get(&self, uri: &Url) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Get a mutable document by URI
    pub fn get_mut(&mut self, uri: &Url) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    /// Check if a document is open
    pub fn is_open(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    /// Get all open documents
    pub fn all_documents(&self) -> Vec<&Document> {
        self.documents.values().collect()
    }

    /// Get the number of open documents
    pub fn count(&self) -> usize {
        self.documents.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn test_open_document() {
        let mut manager = DocumentManager::new();
        let uri = make_url("file:///test.hc");

        manager.open(uri.clone(), "content".to_string(), 1, "hc".to_string());

        assert!(manager.is_open(&uri));
        assert_eq!(manager.count(), 1);

        let doc = manager.get(&uri).unwrap();
        assert_eq!(doc.content, "content");
        assert_eq!(doc.version, 1);
        assert_eq!(doc.language_id, "hc");
    }

    #[test]
    fn test_close_document() {
        let mut manager = DocumentManager::new();
        let uri = make_url("file:///test.hc");

        manager.open(uri.clone(), "content".to_string(), 1, "hc".to_string());
        assert!(manager.is_open(&uri));

        let closed = manager.close(&uri);
        assert!(closed.is_some());
        assert!(!manager.is_open(&uri));
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_update_document() {
        let mut manager = DocumentManager::new();
        let uri = make_url("file:///test.hc");

        manager.open(uri.clone(), "content".to_string(), 1, "hc".to_string());

        let updated = manager.update(&uri, "new content".to_string(), 2);
        assert!(updated);

        let doc = manager.get(&uri).unwrap();
        assert_eq!(doc.content, "new content");
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn test_update_nonexistent_document() {
        let mut manager = DocumentManager::new();
        let uri = make_url("file:///test.hc");

        let updated = manager.update(&uri, "content".to_string(), 1);
        assert!(!updated);
    }

    #[test]
    fn test_multiple_documents() {
        let mut manager = DocumentManager::new();
        let uri1 = make_url("file:///test1.hc");
        let uri2 = make_url("file:///test2.hc");

        manager.open(uri1.clone(), "content1".to_string(), 1, "hc".to_string());
        manager.open(uri2.clone(), "content2".to_string(), 1, "hc".to_string());

        assert_eq!(manager.count(), 2);
        assert!(manager.is_open(&uri1));
        assert!(manager.is_open(&uri2));

        let docs = manager.all_documents();
        assert_eq!(docs.len(), 2);
    }
}
