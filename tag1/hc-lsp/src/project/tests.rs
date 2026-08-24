//! hc-lsp/src/project/tests.rs

use super::*;

#[test]
fn test_new_project_context() {
    let ctx = ProjectContext::new();
    assert!(ctx.root_uri.is_none());
    assert!(ctx.root_path.is_none());
    assert!(!ctx.has_root());
}

#[test]
fn test_set_root_uri() {
    let mut ctx = ProjectContext::new();
    let uri = Url::parse("file:///test/project").unwrap();

    ctx.set_root_uri(uri.clone());

    assert!(ctx.has_root());
    assert_eq!(ctx.root_uri(), Some(&uri));
}

#[test]
fn test_find_build_zon() {
    let mut ctx = ProjectContext::new();
    let path = PathBuf::from("/test/project");

    ctx.set_root_path(path);

    let build_zon = ctx.find_build_zon();
    assert!(build_zon.is_some());
    assert_eq!(build_zon.unwrap(), PathBuf::from("/test/project/build.zon"));
}
