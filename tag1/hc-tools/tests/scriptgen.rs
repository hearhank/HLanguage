//! 组 B：script 块装载期展开端到端测试（ADR-0013，E1.1）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，覆盖：
//!   - B1/B3：产物 = 代码字符串就地替换（声明级文本区间）；多轮展开
//!   - B2：`types` 元数据对象（fields / all / type，Q23）
//!   - B4：受限 H 核心子集负例（io / alloc / 非字符串产物 → 编译错误）
//!   - B5：`hc run`（tree-walking）与 `hc run --ir` 一致装载

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn hc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hc"))
}

/// 独立临时目录（每个测试独占，避免兄弟文件扫描/残留干扰；测后清理）
fn temp_dir(tag: &str) -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hc_scriptgen_{}_{}_{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, src: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    path
}

fn run_hc(args: &[&Path]) -> Output {
    let mut cmd = Command::new(hc_bin());
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("run hc")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn minimal_script_string_product_replaces_block() {
    // B1/B3：script 块产物 = 字符串，就地替换块文本区间；生成 fn 可调用
    let dir = temp_dir("minimal");
    let file = write(
        &dir,
        "minimal.hc",
        "import H.std.{io};\n\
         script { \"fn generated() i32 { return 42; }\"; }\n\
         fn main(args: o Vec<String>) !void {\n\
         \x20   io.print(\"generated = {}\\n\", generated());\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "script 生成应运行成功: {s}");
    assert!(s.contains("generated = 42"), "应调用生成函数: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn types_fields_drive_generation() {
    // B2：types.fields(name) → [["字段名","类型串"],...]；for 遍历 + .concat 拼接产物
    let dir = temp_dir("fields");
    let file = write(
        &dir,
        "fields.hc",
        "import H.std.{io};\n\
         class Person {\n\
         \x20   name: String,\n\
         \x20   age: i32,\n\
         }\n\
         script {\n\
         \x20   var count = 0;\n\
         \x20   var out = \"\";\n\
         \x20   for (types.fields(\"Person\")) |f| {\n\
         \x20       count = count + 1;\n\
         \x20       out = out.concat(\"// \")\n\
         \x20           .concat(f[0]).concat(\": \").concat(f[1]).concat(\"\\n\");\n\
         \x20   }\n\
         \x20   out.concat(\"fn person_field_count() i32 { return \")\n\
         \x20       .concat(String.from(count)).concat(\"; }\");\n\
         }\n\
         fn main(args: o Vec<String>) !void {\n\
         \x20   io.print(\"count = {}\\n\", person_field_count());\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "types.fields 生成应运行成功: {s}");
    assert!(s.contains("count = 2"), "生成函数应返回字段数 2: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn types_all_and_type_metadata() {
    // B2：types.all → 可见类型清单；types.type → 当前类型名（顶层 = ""）
    let dir = temp_dir("meta");
    let file = write(
        &dir,
        "meta.hc",
        "import H.std.{io};\n\
         class Widget { size: i32, }\n\
         script {\n\
         \x20   var names = \"\";\n\
         \x20   for (types.all) |n| {\n\
         \x20       names = names.concat(n).concat(\",\");\n\
         \x20   }\n\
         \x20   \"// types: \".concat(names).concat(\"| type=\").concat(types.type);\n\
         }\n\
         fn main(args: o Vec<String>) !void { io.print(\"ok\\n\"); }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "types.all/type 应运行成功: {s}");
    assert!(s.contains("ok"), "应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multi_round_expansion_later_script_sees_earlier_output() {
    // B3：多轮展开——后块可引用前块生成物；命名空间内 script 块也被展开
    let dir = temp_dir("multi");
    let file = write(
        &dir,
        "multi.hc",
        "import H.std.{io};\n\
         namespace Gen {\n\
         \x20   script { \"fn inner() i32 { return 7; }\"; }\n\
         }\n\
         script { \"fn total() i32 { return Gen.inner(); }\"; }\n\
         fn main(args: o Vec<String>) !void {\n\
         \x20   io.print(\"total = {}\\n\", total());\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "多轮展开应运行成功: {s}");
    assert!(s.contains("total = 7"), "后块应引用前块生成物: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_mode_expands_scripts() {
    // B3：hc check 走同一装载路径（script 展开后解析通过）
    let dir = temp_dir("check");
    let file = write(
        &dir,
        "check.hc",
        "script { \"fn generated() i32 { return 1; }\"; }\n\
         fn main(args: o Vec<String>) !void { _ = generated(); }\n",
    );
    let out = run_hc(&[Path::new("check"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "check 应通过: {s}");
    assert!(s.contains("OK"), "check 应输出 OK: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ir_mode_consistent_loading() {
    // B5：--ir 路径对展开后 AST 一致装载（产物替换在降级前完成，后端无感知）
    let dir = temp_dir("ir");
    let file = write(
        &dir,
        "ir.hc",
        "import H.std.{io};\n\
         class Point { x: i32, y: i32, }\n\
         script {\n\
         \x20   var out = \"\";\n\
         \x20   for (types.fields(\"Point\")) |f| {\n\
         \x20       out = out.concat(\"// \").concat(f[0]).concat(\"\\n\");\n\
         \x20   }\n\
         \x20   out.concat(\"fn fields_str() String { return \\\"\\\"; }\");\n\
         }\n\
         fn main(args: o Vec<String>) !void { io.print(\"ir ok\\n\"); }\n",
    );
    let out = run_hc(&[Path::new("run"), &Path::new("--ir"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "--ir 应一致装载: {s}");
    assert!(s.contains("ir ok"), "--ir 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_mode_collects_generated_test_fn() {
    // B5：`hc test` 装载路径同样先展开 script；生成 fn 可被 [test] 引用
    let dir = temp_dir("testmode");
    let file = write(
        &dir,
        "testmode.hc",
        "script { \"fn answer() i32 { return 42; }\"; }\n\
         [test] fn generated_fn_visible() !void { try expect(answer() == 42); }\n",
    );
    let out = run_hc(&[Path::new("test"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "test 模式 script 展开应通过: {s}");
    assert!(s.contains("[PASS] generated_fn_visible"), "生成 fn 应被测试引用: {s}");
    assert!(s.contains("1 passed, 0 failed, 0 skipped"), "应汇总通过: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn script_io_forbidden() {
    // B4：受限子集负例——script 块内 io 调用 → error.ScriptForbidden（编译错误）
    let dir = temp_dir("neg_io");
    let file = write(
        &dir,
        "neg_io.hc",
        "script {\n\
         \x20   io.print(\"nope\\n\");\n\
         \x20   \"fn x() i32 { return 1; }\";\n\
         }\n\
         fn main(args: o Vec<String>) !void {}\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stderr(&out);
    assert!(!out.status.success(), "script 内 io 应失败: {s}");
    assert!(s.contains("ScriptForbidden"), "应报 ScriptForbidden: {s}");
    assert!(s.contains("io 不可用"), "应指明 io 受限: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn script_alloc_forbidden() {
    // B4：受限子集负例——script 块内 alloc 根 → error.ScriptForbidden
    let dir = temp_dir("neg_alloc");
    let file = write(
        &dir,
        "neg_alloc.hc",
        "script {\n\
         \x20   var arena = alloc;\n\
         \x20   \"fn x() i32 { return 1; }\";\n\
         }\n\
         fn main(args: o Vec<String>) !void {}\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stderr(&out);
    assert!(!out.status.success(), "script 内 alloc 应失败: {s}");
    assert!(s.contains("ScriptForbidden"), "应报 ScriptForbidden: {s}");
    assert!(s.contains("alloc 不可用"), "应指明 alloc 受限: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn serialization_boilerplate_validate_and_to_json() {
    // C1（Q37/Q38 定制通道）：脚本从 types.fields 生成校验 + to_json 样板。
    // 校验规则（类型驱动）：String 非空 / i32 >= 0 / ?String null 允许、非空时非空串；
    // to_json：String 带引号、i32 裸值（fmt_int）、?String null → `null`。
    // 端到端 `hc run` 与 `hc run --ir` 输出一致（B5 后端无感知）。
    let dir = temp_dir("c1_ser");
    let file = write(
        &dir,
        "ser.hc",
        r#"import H.std.{io};

class User {
    name: String,
    age: i32,
    email: ?String,
}

script {
    var out = "fn user_validate(u: *User) !void {\n";
    for (types.fields("User")) |f| {
        if (f[1] == "String") {
            out = out.concat("    if (u.").concat(f[0]).concat(".len() == 0) return error.Invalid;\n");
        } else if (f[1] == "i32") {
            out = out.concat("    if (u.").concat(f[0]).concat(" < 0) return error.Invalid;\n");
        } else if (f[1] == "?String") {
            out = out.concat("    if (u.").concat(f[0]).concat(" != null) {\n");
            out = out.concat("        if (u.").concat(f[0]).concat(".?.len() == 0) return error.Invalid;\n");
            out = out.concat("    }\n");
        }
    }
    out = out.concat("}\n");

    out = out.concat("fn user_to_json(u: *User, alloc: Allocator) String {\n");
    out = out.concat("    var out = \"{\";\n");
    var first = true;
    for (types.fields("User")) |f| {
        var sep = "";
        if (first) {
            first = false;
        } else {
            sep = ", ";
        }
        if (f[1] == "String") {
            out = out.concat("    out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \\\"\").concat(u.")
                .concat(f[0])
                .concat(").concat(\"\\\"\");\n");
        } else if (f[1] == "i32") {
            out = out.concat("    out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \").concat(fmt_int(u.")
                .concat(f[0])
                .concat("));\n");
        } else if (f[1] == "?String") {
            out = out.concat("    if (u.").concat(f[0]).concat(" != null) {\n");
            out = out.concat("        out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": \\\"\").concat(u.")
                .concat(f[0])
                .concat(".?).concat(\"\\\"\");\n");
            out = out.concat("    } else {\n");
            out = out.concat("        out = out.concat(\"")
                .concat(sep)
                .concat("\\\"")
                .concat(f[0])
                .concat("\\\": null\");\n");
            out = out.concat("    }\n");
        }
    }
    out = out.concat("    out = out.concat(\"}\");\n");
    out = out.concat("    return out;\n");
    out = out.concat("}\n");
    out;
}

fn main(args: o Vec<String>) !void {
    var good = alloc.init(User{name = "alice", age = 30, email = "a@x.com"});
    try user_validate(&good);
    io.print("json = {}\n", user_to_json(&good, alloc));
    var bad = alloc.init(User{name = "bob", age = -1, email = null});
    user_validate(&bad) catch |e| {
        io.print("bad = {}\n", e);
    };
    var none = alloc.init(User{name = "carol", age = 40, email = null});
    io.print("none = {}\n", user_to_json(&none, alloc));
}
"#,
    );
    let expected = r#"json = {"name": "alice", "age": 30, "email": "a@x.com"}
bad = error.Invalid
none = {"name": "carol", "age": 40, "email": null}"#;

    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "tree-walking 应成功: {s}");
    assert_eq!(s.trim(), expected, "校验+to_json 样板输出应正确: {s}");

    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    let s_ir = stdout(&out_ir);
    assert!(out_ir.status.success(), "--ir 应成功: {s_ir}");
    assert_eq!(s_ir.trim(), expected, "IR 输出应与 tree-walking 一致: {s_ir}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_string_product_rejected() {
    // B3：产物非字符串 → 编译错误（带值种类提示）
    let dir = temp_dir("neg_int");
    let file = write(
        &dir,
        "neg_int.hc",
        "script { 42; }\n\
         fn main(args: o Vec<String>) !void {}\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stderr(&out);
    assert!(!out.status.success(), "非字符串产物应失败: {s}");
    assert!(s.contains("必须是字符串"), "应提示产物必须为字符串: {s}");
    assert!(s.contains("整数"), "应提示得到整数: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}
