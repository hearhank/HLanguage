//! 组 D D2：`comptime { }` 块编译期求值端到端测试（ADR-0012，E1.2）。
//!
//! 用 `hc` 二进制（CARGO_BIN_EXE_hc-tools）驱动 CLI，覆盖最小切片：
//!   - 块通过：`types` 全量可见（all / fields），求值后丢弃、正常进入运行
//!   - 块失败：`return error.X` = 编译错误（带块内位置 + 所属块位置）
//!   - 负例：`types.fields` 未知类型 / io 禁用 → 编译错误
//!   - 顺序：script 展开后求值——comptime 块可见 script 生成类型

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
    let dir =
        std::env::temp_dir().join(format!("hc_comptime_{}_{}_{}", std::process::id(), tag, n));
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

/// stdout + stderr 合并（编译错误诊断可能在任一流）
fn combined(out: &Output) -> String {
    format!("{}\n{}", stdout(out), stderr(out))
}

#[test]
fn comptime_block_passes_and_program_runs() {
    // 块通过：types 全量可见（all / fields），求值结果丢弃，main 正常执行
    let dir = temp_dir("pass");
    let file = write(
        &dir,
        "pass.hc",
        "import H.std.{io};\n\
         class User { name: String }\n\
         comptime {\n\
         \x20   if (types.all.len() < 1) { return error.NoTypes; }\n\
         \x20   if (types.fields(\"User\").len() != 1) { return error.BadSchema; }\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"comptime ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime 块通过应运行成功: {}",
        combined(&out)
    );
    assert!(s.contains("comptime ok"), "main 应正常执行: {s}");
    // 三后端一致：IR 装载同样通过
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    assert!(
        out_ir.status.success(),
        "IR 装载应一致通过: {}",
        combined(&out_ir)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_return_error_is_compile_error() {
    // 块失败：return error.X = 编译错误（带块内位置 + 错误名）
    let dir = temp_dir("fail");
    let file = write(
        &dir,
        "fail.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   return error.BadSchema;\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(
        !out.status.success(),
        "comptime 块返回 error 应为编译失败: {s}"
    );
    assert!(s.contains("BadSchema"), "诊断应含错误名: {s}");
    assert!(
        s.contains("comptime 块返回错误"),
        "诊断应说明块返回错误: {s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_unknown_type_fields_is_compile_error() {
    // 负例：types.fields 未知类型 → UnknownType 编译错误
    let dir = temp_dir("unknown");
    let file = write(
        &dir,
        "unknown.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var f = types.fields(\"NoSuchType\");\n\
         \x20   _ = f;\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "未知类型应为编译失败: {s}");
    assert!(s.contains("UnknownType"), "诊断应含 UnknownType: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_io_forbidden() {
    // 负例：comptime 块中 io 不可用（受限 H 核心子集，同 script）
    let dir = temp_dir("io");
    let file = write(
        &dir,
        "io.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x = io;\n\
         \x20   _ = x;\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "comptime 块中 io 应为编译失败: {s}");
    assert!(s.contains("io 不可用"), "诊断应说明 io 受限: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_sees_script_generated_types() {
    // 顺序：script 展开后求值——comptime 块可见 script 生成的 class
    let dir = temp_dir("script");
    let file = write(
        &dir,
        "script.hc",
        "import H.std.{io};\n\
         class Point { x: i32 }\n\
         script {\n\
         \x20   var fields = types.fields(\"Point\");\n\
         \x20   var out = \"class Gen { \";\n\
         \x20   for (fields) |f| {\n\
         \x20       out = out.concat(\"gen_\").concat(f[0]).concat(\": \")\n\
         \x20           .concat(f[1]).concat(\", \");\n\
         \x20   }\n\
         \x20   out.concat(\"}\");\n\
         }\n\
         comptime {\n\
         \x20   if (types.fields(\"Gen\").len() < 1) { return error.NoGen; }\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"script+comptime ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime 应见 script 生成类型: {}",
        combined(&out)
    );
    assert!(s.contains("script+comptime ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 组 D D4：comptime_int 常量折叠（类型层） ----------

#[test]
fn comptime_arith_folding_expect_eq_passes() {
    // 折叠 + 类型检查双通过：comptime_int 算术在装载期求值，expect_eq 断言成立
    let dir = temp_dir("fold");
    let file = write(
        &dir,
        "fold.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: comptime_int = 1 + 2;\n\
         \x20   expect_eq(x, 3);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"fold ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime_int 折叠应通过: {}",
        combined(&out)
    );
    assert!(s.contains("fold ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_folding_expect_eq_failure_is_compile_error() {
    // 负例：折叠断言不成立 → 装载期 AssertFailed = 编译错误
    let dir = temp_dir("foldfail");
    let file = write(
        &dir,
        "foldfail.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   expect_eq(1 + 2, 4);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "折叠断言失败应为编译错误: {s}");
    assert!(s.contains("AssertFailed"), "诊断应含 AssertFailed: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_int_var_decl_typechecks_and_folds() {
    // comptime_int 变量声明类型检查 + 算术折叠
    let dir = temp_dir("varfold");
    let file = write(
        &dir,
        "varfold.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: comptime_int = 40 + 2;\n\
         \x20   expect_eq(x, 42);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"varfold ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime_int 变量折叠应通过: {}",
        combined(&out)
    );
    assert!(s.contains("varfold ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_narrowing_u8_overflow_is_compile_error() {
    // 负例：comptime 块内收窄溢出（u8 = 256）→ 语义检查在收窄点诊断 = 编译错误
    let dir = temp_dir("narrow");
    let file = write(
        &dir,
        "narrow.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: u8 = 256;\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(
        !out.status.success(),
        "comptime 块内收窄溢出应为编译错误: {s}"
    );
    assert!(s.contains("out of range"), "诊断应含 out of range: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_narrowing_u8_in_range_passes() {
    // comptime 块内收窄在范围内 → 通过
    let dir = temp_dir("narrowok");
    let file = write(
        &dir,
        "narrowok.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: u8 = 200;\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"narrow ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime 块内范围内收窄应通过: {}",
        combined(&out)
    );
    assert!(s.contains("narrow ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_float_folding_expect_eq_passes() {
    // comptime_float 折叠 + 类型检查双通过：浮点算术在装载期求值，expect_eq 断言成立
    let dir = temp_dir("floatfold");
    let file = write(
        &dir,
        "floatfold.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: comptime_float = 1.5 + 2.5;\n\
         \x20   expect_eq(x, 4.0);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"floatfold ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime_float 折叠应通过: {}",
        combined(&out)
    );
    assert!(s.contains("floatfold ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_float_folding_expect_eq_failure_is_compile_error() {
    // 负例：浮点折叠断言不成立 → 装载期 AssertFailed = 编译错误
    let dir = temp_dir("floatfail");
    let file = write(
        &dir,
        "floatfail.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   expect_eq(1.5 + 2.5, 3.0);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "浮点折叠断言失败应为编译错误: {s}");
    assert!(s.contains("AssertFailed"), "诊断应含 AssertFailed: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_float_var_decl_typechecks_and_folds() {
    // comptime_float 变量声明类型检查 + 算术折叠（除法精确：40.0/2.0 = 20.0）
    let dir = temp_dir("vardiv");
    let file = write(
        &dir,
        "vardiv.hc",
        "import H.std.{io};\n\
         comptime {\n\
         \x20   var x: comptime_float = 40.0 / 2.0;\n\
         \x20   expect_eq(x, 20.0);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"vardiv ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime_float 变量折叠应通过: {}",
        combined(&out)
    );
    assert!(s.contains("vardiv ok"), "main 应正常执行: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 组 D D4b：anytype 完整语义（调用点具体化） ----------

#[test]
fn anytype_ret_resolves_concrete_across_modes() {
    // anytype 调用点具体化 + 三后端一致：`max_value` 返回类型在调用点解析为具体
    // 类型（f64 / i32），运行时动态分派不受影响——interp 与 IR 结果一致
    let dir = temp_dir("anytype");
    let file = write(
        &dir,
        "anytype.hc",
        "import H.std.{io};\n\
         fn max_value(a: anytype, b: anytype) anytype {\n\
         \x20   return if (a > b) a else b;\n\
         }\n\
         fn main() !void {\n\
         \x20   var m: f64 = max_value(2.5, 1.5);\n\
         \x20   io.print(\"max = {}\\n\", m);\n\
         \x20   var n: i32 = max_value(3, 7);\n\
         \x20   io.print(\"max = {}\\n\", n);\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "anytype 具体化应通过: {}",
        combined(&out)
    );
    assert!(s.contains("max = 2.5"), "f64 实例结果: {s}");
    assert!(s.contains("max = 7"), "i32 实例结果: {s}");
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    let s_ir = stdout(&out_ir);
    assert!(
        out_ir.status.success(),
        "IR 装载应一致通过: {}",
        combined(&out_ir)
    );
    assert_eq!(s, s_ir, "interp 与 IR 输出应一致");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn anytype_ret_concrete_mismatch_is_compile_error() {
    // 负例：anytype 返回类型具体化为 f64 后，赋给 String → 语义检查诊断 = 编译错误
    // （判别场景：具体化前返回类型为 anytype 通配，此赋值被静默放行）
    let dir = temp_dir("anytypemismatch");
    let file = write(
        &dir,
        "mismatch.hc",
        "import H.std.{io};\n\
         fn max_value(a: anytype, b: anytype) anytype {\n\
         \x20   return if (a > b) a else b;\n\
         }\n\
         fn main() !void {\n\
         \x20   var s: String = max_value(2.5, 1.5);\n\
         \x20   io.print(\"{}\\n\", s);\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(
        !out.status.success(),
        "f64 结果赋给 String 应为编译失败: {s}"
    );
    assert!(s.contains("cannot assign"), "诊断应含类型不匹配: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- 组 D D4c：comptime 值函数（参数含 `T: type`，调用点编译期求值） ----------

#[test]
fn comptime_value_fn_type_param_folds_in_block() {
    // 值函数（参数含 `T: type`、返回 comptime_int，非返回 type 的类型函数）在 comptime
    // 块内调用 → 装载期求值折叠为常量；interp 与 IR 装载一致（comptime 块跳过，定义可降级）
    let dir = temp_dir("valfn");
    let file = write(
        &dir,
        "valfn.hc",
        "import H.std.{io};\n\
         fn array_len(T: type) comptime_int {\n\
         \x20   return 4;\n\
         }\n\
         comptime {\n\
         \x20   var a: comptime_int = array_len(i32);\n\
         \x20   expect_eq(a, 4);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"valfn ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "comptime 值函数应折叠: {}",
        combined(&out)
    );
    assert!(s.contains("valfn ok"), "main 应正常执行: {s}");
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    assert!(
        out_ir.status.success(),
        "IR 装载应一致通过: {}",
        combined(&out_ir)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_value_fn_mixed_type_and_value_params() {
    // 混合参数：`T: type` + `n: comptime_int` → 类型实参作类型绑定、值实参常量求值
    let dir = temp_dir("mixed");
    let file = write(
        &dir,
        "mixed.hc",
        "import H.std.{io};\n\
         fn byte_size(T: type, n: comptime_int) comptime_int {\n\
         \x20   return n + 1;\n\
         }\n\
         comptime {\n\
         \x20   var a: comptime_int = byte_size(f64, 7);\n\
         \x20   expect_eq(a, 8);\n\
         \x20   var b: comptime_int = byte_size(Vec<i32>, 0);\n\
         \x20   expect_eq(b, 1);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"mixed ok\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(
        out.status.success(),
        "混合参数值函数应折叠: {}",
        combined(&out)
    );
    assert!(s.contains("mixed ok"), "main 应正常执行: {s}");
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    assert!(
        out_ir.status.success(),
        "IR 装载应一致通过: {}",
        combined(&out_ir)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_value_fn_runtime_call_folds_in_interp() {
    // 运行时调用点：`var n: comptime_int = array_len(i32);` 折叠为常量——
    // interp（D4c）与 IR（D5 `value_fns` 常量求值）一致，IR 中无类型值/调用残留
    let dir = temp_dir("runtime");
    let file = write(
        &dir,
        "runtime.hc",
        "import H.std.{io};\n\
         fn array_len(T: type) comptime_int {\n\
         \x20   return 4;\n\
         }\n\
         fn byte_size(T: type, n: comptime_int) comptime_int {\n\
         \x20   return n + 1;\n\
         }\n\
         fn main() !void {\n\
         \x20   var n: comptime_int = array_len(i32);\n\
         \x20   io.print(\"n = {}\\n\", n);\n\
         \x20   var m: comptime_int = byte_size(f64, 7);\n\
         \x20   io.print(\"m = {}\\n\", m);\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = stdout(&out);
    assert!(out.status.success(), "运行时调用应折叠: {}", combined(&out));
    assert!(s.contains("n = 4"), "折叠结果: {s}");
    assert!(s.contains("m = 8"), "混合参数折叠结果: {s}");
    let out_ir = run_hc(&[Path::new("run"), Path::new("--ir"), &file]);
    let s_ir = stdout(&out_ir);
    assert!(
        out_ir.status.success(),
        "IR 运行时调用应折叠: {}",
        combined(&out_ir)
    );
    assert!(
        s_ir.contains("n = 4") && s_ir.contains("m = 8"),
        "IR 折叠结果: {s_ir}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_value_fn_self_recursion_is_compile_error() {
    // 负例：自递归 comptime 值函数（`loop_fn(i32)` 体再调自身 → 无限编译期求值）
    // → ComptimeRecursion = 编译错误（深度守卫）
    let dir = temp_dir("recursion");
    let file = write(
        &dir,
        "recursion.hc",
        "import H.std.{io};\n\
         fn loop_fn(T: type) comptime_int {\n\
         \x20   return loop_fn(i32);\n\
         }\n\
         comptime {\n\
         \x20   var x: comptime_int = loop_fn(i32);\n\
         \x20   expect_eq(x, 1);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "自递归应为编译失败: {s}");
    assert!(
        s.contains("ComptimeRecursion"),
        "诊断应含 ComptimeRecursion: {s}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comptime_value_fn_non_type_arg_is_compile_error() {
    // 负例：`T: type` 实参不是已知类型（未定义名 `mystery`）→ 回落既有调用路径
    // → UndefinedName = 编译错误
    let dir = temp_dir("badarg");
    let file = write(
        &dir,
        "badarg.hc",
        "import H.std.{io};\n\
         fn array_len(T: type) comptime_int {\n\
         \x20   return 4;\n\
         }\n\
         comptime {\n\
         \x20   var x: comptime_int = array_len(mystery);\n\
         \x20   expect_eq(x, 4);\n\
         }\n\
         fn main() !void {\n\
         \x20   io.print(\"unreachable\\n\");\n\
         }\n",
    );
    let out = run_hc(&[Path::new("run"), &file]);
    let s = combined(&out);
    assert!(!out.status.success(), "非类型实参应为编译失败: {s}");
    assert!(s.contains("UndefinedName"), "诊断应含 UndefinedName: {s}");
    let _ = std::fs::remove_dir_all(&dir);
}
