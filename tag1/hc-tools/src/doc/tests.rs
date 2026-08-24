//! 文档生成器测试：doc 命令输出验证

use super::*;

#[test]
fn doc_run_extraction() {
    let src = "/// 文件头\n/// 第二行\n\n/// fn 文档\nfn f() i32 { return 1; }\n";
    let runs = collect_doc_runs(src);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text, "文件头\n第二行");
    assert_eq!(runs[1].text, "fn 文档");
}

#[test]
fn page_contains_sig_and_doc() {
    let src =
        "import H.std.{io};\n\n/// 入口函数\nfn main() !void {\n    io.print(\"hi\\n\");\n}\n";
    let page = render_file_page("main.hc", src).unwrap();
    assert!(page.contains("# `main`"), "page: {page}");
    assert!(page.contains("fn main() !void"));
    assert!(page.contains("入口函数"));
    assert!(page.contains("import H.std.{io};"));
}

#[test]
fn stdlib_page_has_modules() {
    let mut out = String::new();
    for (module, _) in STDLIB {
        out.push_str(module);
    }
    assert!(out.contains("io（I/O）"));
    assert!(out.contains("线程（组 G 生命周期）"));
}

#[test]
fn render_type_covers_forms() {
    assert_eq!(render_type(&Type::Named("i32".into(), vec![])), "i32");
    assert_eq!(
        render_type(&Type::Named(
            "Vec".into(),
            vec![Type::Named("i32".into(), vec![])]
        )),
        "Vec<i32>"
    );
    assert_eq!(
        render_type(&Type::Owned(Box::new(Type::Named("String".into(), vec![])))),
        "o String"
    );
    assert_eq!(
        render_type(&Type::ErrorUnion(
            None,
            Box::new(Type::Named("void".into(), vec![]))
        )),
        "!void"
    );
    assert_eq!(
        render_type(&Type::Array(2, Box::new(Type::Named("i32".into(), vec![])))),
        "[2]i32"
    );
}
