//! hc-rt/tests/import.rs

use hc_rt::Interp;

fn parse(s: &str) -> hc::Program {
    hc::parse_source(s).unwrap_or_else(|d| panic!("parse: {:?}", d))
}

fn run_tests_ok(src: &str) -> Interp {
    let program = parse(src);
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed tests: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
    interp
}

fn run_dep_tests_ok(dep_src: &str, main_src: &str) -> Interp {
    let dep = parse(dep_src);
    let main = parse(main_src);
    let mut interp = Interp::new("");
    interp
        .load_dep("jsonlib", &[&dep])
        .unwrap_or_else(|e| panic!("load_dep: {} {}", e.name, e.message));
    interp
        .load(&main)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
    interp
}

#[test]
fn import_io_symbol_selection_alias() {
    // `import H.std.{io as my};` → `my.print` 走 io 环境对象分发（test_out 捕获输出）
    let interp = run_tests_ok(
        "import H.std.{io as my};\n\
         [test] fn t() !void {\n\
             my.print(\"hello-import\\n\");\n\
         }\n",
    );
    assert!(
        interp.test_out.iter().any(|l| l.contains("hello-import")),
        "test_out: {:?}",
        interp.test_out
    );
}

#[test]
fn import_io_whole_module_alias() {
    // `import H.std.io as out;` → 整模块 + 别名 → `out.print`
    let interp = run_tests_ok(
        "import H.std.io as out;\n\
         [test] fn t() !void {\n\
             out.print(\"hello-whole\\n\");\n\
         }\n",
    );
    assert!(
        interp.test_out.iter().any(|l| l.contains("hello-whole")),
        "test_out: {:?}",
        interp.test_out
    );
}

#[test]
fn import_io_whole_module_no_alias() {
    // `import H.std.io;` → 绑定名 = 末段 `io`
    let interp = run_tests_ok(
        "import H.std.io;\n\
         [test] fn t() !void {\n\
             io.print(\"hello-io\\n\");\n\
         }\n",
    );
    assert!(
        interp.test_out.iter().any(|l| l.contains("hello-io")),
        "test_out: {:?}",
        interp.test_out
    );
}

#[test]
fn import_user_pkg_whole_module_alias() {
    // 用户包整模块 + 别名：`import jsonlib as j;` → `j.double(21)`
    run_dep_tests_ok(
        "pub fn double(x: i32) i32 { return x * 2; }\n",
        "import jsonlib as j;\n\
         [test] fn t() !void {\n\
             var a = j.double(21);\n\
             try expect_eq(a, 42);\n\
         }\n",
    );
}

#[test]
fn import_user_pkg_whole_module_no_alias() {
    // 用户包整模块（无别名）：`import jsonlib;` → `jsonlib.double(21)`
    run_dep_tests_ok(
        "pub fn double(x: i32) i32 { return x * 2; }\n",
        "import jsonlib;\n\
         [test] fn t() !void {\n\
             var a = jsonlib.double(21);\n\
             try expect_eq(a, 42);\n\
         }\n",
    );
}

#[test]
fn import_user_pkg_symbol_selection_alias() {
    // 符号选择 + as：`import jsonlib.{double as dbl};` → `dbl(21)` 直调
    run_dep_tests_ok(
        "pub fn double(x: i32) i32 { return x * 2; }\n",
        "import jsonlib.{double as dbl};\n\
         [test] fn t() !void {\n\
             var a = dbl(21);\n\
             try expect_eq(a, 42);\n\
         }\n",
    );
}

#[test]
fn import_user_pkg_symbol_selection_no_alias() {
    // 符号选择（无别名）：`import jsonlib.{double};` → `double(21)` 直调
    run_dep_tests_ok(
        "pub fn double(x: i32) i32 { return x * 2; }\n",
        "import jsonlib.{double};\n\
         [test] fn t() !void {\n\
             var a = double(21);\n\
             try expect_eq(a, 42);\n\
         }\n",
    );
}

#[test]
fn import_own_def_wins() {
    // 文件自身定义优先：import 绑定不覆盖自身函数
    let interp = run_tests_ok(
        "import H.std.{io as my};\n\
         fn double(x: i32) i32 { return x * 2; }\n\
         [test] fn t() !void {\n\
             try expect_eq(double(21), 42);\n\
         }\n",
    );
    assert!(
        interp.test_out.iter().any(|l| l.contains("[PASS]")),
        "test_out: {:?}",
        interp.test_out
    );
}

// ---------- A2b：模块识别（[module]）运行时隔离 ----------

/// 运行测试并断言恰好 1 个失败（负例）
fn run_tests_one_fail(src: &str) -> Interp {
    let program = parse(src);
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 1, "预期 1 个失败: {:?}", interp.test_out);
    assert_eq!(p, 0, "不应有通过: {:?}", interp.test_out);
    interp
}

#[test]
fn a2b_namespace_qualified_access_works() {
    // 命名空间成员限定访问 `M.f()` 可用
    run_tests_ok(
        "namespace M { pub fn f() i32 { return 1; } }\n[test] fn t() !void {\n    var x = M.f();\n    try expect_eq(x, 1);\n}\n",
    );
}

#[test]
fn a2b_non_module_namespace_flat_access_works() {
    // 普通命名空间成员仍扁平可用（隔离仅限 `[module]`）
    run_tests_ok(
        "namespace N { fn f() i32 { return 1; } }\n[test] fn t() !void {\n    var x = f();\n    try expect_eq(x, 1);\n}\n",
    );
}
