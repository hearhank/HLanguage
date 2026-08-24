use super::*;

#[test]
fn parse_full_manifest() {
    let src = r#"
const build = Build{
    name = "orders",
    version = "0.1.0",
    kind = Kind.exe,
    files = [ "main.hc", "math.hc", ],
    deps = [
        Pkg{ name = "json", version = "0.2.0", fingerprint = 0xa1b2, path = "../json" },
    ],
};
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.name, "orders");
    assert_eq!(m.version, "0.1.0");
    assert_eq!(m.kind, Kind::Exe);
    assert_eq!(m.files, vec!["main.hc", "math.hc"]);
    assert_eq!(m.deps.len(), 1);
    let d = &m.deps[0];
    assert_eq!(d.name, "json");
    assert_eq!(d.version, "0.2.0");
    assert_eq!(d.fingerprint, Some("a1b2".into()));
    assert_eq!(d.path.as_deref(), Some(Path::new("../json")));
}

#[test]
fn parse_dep_without_path_is_registry() {
    let src = r#"
const build = Build{ name = "a", version = "0.1.0", deps = [ Pkg{ name = "json", version = "0.2.0" } ] };
"#;
    let m = parse(src).unwrap();
    assert_eq!(m.deps[0].path, None);
    assert_eq!(m.deps[0].fingerprint, None);
}

#[test]
fn parse_kinds() {
    for (kind, want) in [
        ("Kind.exe", Kind::Exe),
        ("Kind.lib", Kind::Lib),
        ("Kind.script", Kind::Script),
    ] {
        let src = format!("const build = Build{{ name = \"a\", kind = {kind} }};");
        assert_eq!(parse(&src).unwrap().kind, want);
    }
}

#[test]
fn missing_build_const_errors() {
    assert!(parse("const x = 1;").is_err());
}

#[test]
fn load_from_dir_none_when_absent() {
    let m = load_from_dir(Path::new("/nonexistent/dir/xyz")).unwrap();
    assert!(m.is_none());
}
