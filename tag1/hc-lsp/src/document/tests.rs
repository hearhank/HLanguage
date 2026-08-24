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
