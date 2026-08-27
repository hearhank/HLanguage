//! hc-tools/tests/integration_test.rs

use crate::cli::{color_test_line, paint};
use crate::pkg::{check_and_merge, merge_modules, programs_to_test_ll, strip_test_funcs_in_place};
use crate::project::fsio::{source_to_bytecode, write_bytecode_artifact};
use crate::run::{run_ir_source_with_args, IrRunOutcome};

/// M7.1：入口 + 同包兄弟 → LLVM IR 文本（`main` 入口）。仅测试用（C3 后生产路径
/// 走 [`check_and_merge_deps`] + `codegen_with_links`）。
fn programs_to_ll(
    entry: &hc::Program,
    entry_source: &str,
    siblings: &[&hc::Program],
) -> Result<String, String> {
    let (merged, table) = check_and_merge(entry, entry_source, siblings, false)?;
    Ok(hc::llvm::codegen(&merged, &table))
}

/// 用 IR 参考解释器运行源码入口 `main`（`hc run --ir` 核心，可测试）。
///
/// 流程：解析 → 语义检查（准确优先）→ `lower` → `execute_ir`；失败返回可直接
/// 打印的文本（诊断渲染 / `error.{name}: {message}` + 切片外特性提示）。
/// 不依赖文件系统与退出码——测试用（生产入口走 [`run_ir_source_with_args`]）。
fn run_ir_source(source: &str) -> Result<IrRunOutcome, String> {
    run_ir_source_with_args(source, &[])
}

/// 断言切片内程序运行成功
fn expect_success(src: &str) {
    match run_ir_source(src) {
        Ok(IrRunOutcome::Success) => {}
        other => panic!("预期运行成功，实际：{other:?}"),
    }
}

#[test]
fn slice_in_simple_return() {
    // 零参 main 完整运行：标量 return
    expect_success("fn main() i32 { return 42; }");
}

#[test]
fn slice_in_if_while_try_catch() {
    // 含 if/else-if/while 续步/try/catch/error 字面量的程序
    let src = r#"
fn sum_to(n: i32) i32 {
    var mut i: i32 = 0;
    var mut sum: i32 = 0;
    while (i < n) : (i += 1) { sum += i; }
    return sum;
}
fn fail() !i32 { return error.NotFound; }
fn ok() !i32 { return 5; }
fn pick(x: i32) i32 {
    if (x > 5) { return x; }
    else if (x > 2) { return x * 2; }
    return 0;
}
fn main() i32 {
    var t = try ok();
    var s = fail() catch 7;
    return sum_to(5) + pick(3) + t + s;
}
"#;
    expect_success(src);
}

#[test]
fn main_io_param_void_placeholder() {
    // main(io: Io) 的 io 参数在 IR 下为 Void 占位；未用 io.* 时正常返回
    expect_success("fn main(io: Io) void {}");
}

#[test]
fn unhandled_error_value() {
    // main 返回未处理 error 值（值通道到入口）→ UnhandledError
    let src = "fn main() !i32 { return error.NotFound; }";
    match run_ir_source(src) {
        Ok(IrRunOutcome::UnhandledError(name)) => assert_eq!(name, "NotFound"),
        other => panic!("预期未处理错误，实际：{other:?}"),
    }
}

#[test]
fn ir_io_exit_maps_code() {
    // F2：io.exit 在 IR 侧映射退出码（Exited(code)，对齐 oracle Interp.exit_code）
    let src = "import H.std.{io};\nfn main() !void { io.exit(ExitType.Error, 3); }\n";
    match run_ir_source(src) {
        Ok(IrRunOutcome::Exited(3)) => {}
        other => panic!("预期 Exited(3)，实际：{other:?}"),
    }
    let ok = "import H.std.{io};\nfn main() !void { io.exit(ExitType.Exit, 0); }\n";
    match run_ir_source(ok) {
        Ok(IrRunOutcome::Exited(0)) => {}
        other => panic!("预期 Exited(0)，实际：{other:?}"),
    }
}

#[test]
fn division_by_zero() {
    // 整数除零 → DivisionByZero（对齐 tree-walking arith）
    let src = "fn main() i32 { return 10 / 0; }";
    match run_ir_source(src) {
        Err(msg) => assert!(msg.contains("DivisionByZero"), "消息：{msg}"),
        other => panic!("预期错误，实际：{other:?}"),
    }
}

#[test]
fn no_main_entry() {
    // 无 main 入口 → NoMain（不误导为切片外 NoFunction）
    let src = "fn f() i32 { return 1; }";
    match run_ir_source(src) {
        Err(msg) => assert!(msg.contains("NoMain"), "消息：{msg}"),
        other => panic!("预期 NoMain，实际：{other:?}"),
    }
}

#[test]
fn io_print_through_ir() {
    // Phase 7：io.print 已入 IR 子集——`io` 隐式环境经 LoadGlobal 解析，限定名
    // 调用路由 call_dotted_implicit → call_io_method_ir，成功返回。
    let src = r#"fn main() void { io.print("hi"); }"#;
    match run_ir_source(src) {
        Ok(_) => {}
        other => panic!("预期 Success，实际：{other:?}"),
    }
}

#[test]
fn parse_error_rendered() {
    // 解析失败 → 渲染诊断文本
    assert!(run_ir_source("fn main( {").is_err());
}

#[test]
fn leak_report_does_not_change_exit_code() {
    // G5/§8.3 Debug 泄漏检测：`hc run --ir` 下泄漏程序仍返回 Success——
    // CLI 退出时打印泄漏清单到 stderr，但**不改变退出码**（保持绿，退出码留给 §11）
    expect_success("fn main() void { var buf = alloc.alloc(8); }");
}

#[test]
fn bytecode_source_round_trips() {
    // source_to_bytecode → decode → 重新 encode 字节级一致（覆盖 HBC2 编码确定性）
    let src = "fn main() i32 { return 42; }";
    let bytes = source_to_bytecode(src).expect("encode");
    assert_eq!(&bytes[..4], &hc::bytecode::MAGIC);
    let module = hc::bytecode::decode(&bytes).expect("decode");
    assert_eq!(hc::bytecode::encode(&module), bytes);
}

#[test]
fn write_bytecode_artifact_decodable() {
    // 产物写入后可重新 decode（回退路径的产物是可装载字节码）
    let dir = std::env::temp_dir().join(format!("hc_bc_artifact_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let bytes = source_to_bytecode("fn main() i32 { return 7; }").expect("encode");
    let p = write_bytecode_artifact(&dir, "prog", &bytes).expect("write");
    let read = std::fs::read(&p).expect("read");
    assert!(hc::bytecode::decode(&read).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_modules_exports_qualified_only() {
    // 入口 + 兄弟：兄弟顶层函数扁平名文件私有；命名空间函数限定名导出、索引偏移正确
    let entry =
        hc::ir::lower(&hc::parse_source("fn main() i32 { return Math.square(4); }\n").unwrap())
            .unwrap();
    let sib = hc::ir::lower(
        &hc::parse_source(
            "fn load_config(x: i32) i32 { return x; }\nnamespace Math { fn square(x: i32) i32 { return x * x; } }\n",
        )
        .unwrap(),
    )
    .unwrap();
    let merged = merge_modules(entry, vec![sib]);
    assert!(merged.func_index.contains_key("main"));
    assert!(merged.func_index.contains_key("Math.square"));
    // 兄弟顶层函数扁平名不导出（文件私有）
    assert!(!merged.func_index.contains_key("load_config"));
    // 限定名索引落在追加段（入口在前，偏移后合法）
    assert!(merged.func_index["Math.square"][0] < merged.funcs.len());
    assert!(merged.funcs.len() >= 2);
}

#[test]
fn merge_modules_concats_globals_and_init() {
    // Phase 5 多文件：兄弟 global/const 并入全局表（去重保序），各模块 `@__init__` 保留。
    // IrRuntime::init 预分配全部全局 cell 后按 funcs 序执行各 `@__init__`——同名全局
    // 共享同一 cell，后者覆盖（对齐解释器「后载入覆盖」）。
    let entry = hc::ir::lower(
        &hc::parse_source("global app: i32 = 1;\nfn main() i32 { return app; }\n").unwrap(),
    )
    .unwrap();
    let sib = hc::ir::lower(
        &hc::parse_source("global lib: i32 = 2;\nglobal shared: i32 = 0;\n").unwrap(),
    )
    .unwrap();
    let sib2 = hc::ir::lower(&hc::parse_source("global shared: i32 = 9;\n").unwrap()).unwrap();
    let merged = merge_modules(entry, vec![sib, sib2]);
    // 全局表：声明序 + 去重（同名只保留入口/先序一份）。
    // Phase 7 起隐式环境名（alloc/io/pi/Vec…）也登记全局——入口模块已含，
    // 兄弟并入时去重跳过；用户全局仍保声明序（app → lib → shared）。
    assert_eq!(
        merged.globals,
        vec![
            "app", "alloc", "io", "test_io", "stdout", "stderr", "pi", "Vec", "Deque", "Map",
            "Table", "lib", "shared",
        ]
    );
    // 各模块 `@__init__` 全部保留（funcs 序依次执行）
    let init_count = merged
        .funcs
        .iter()
        .filter(|f| f.name == "@__init__")
        .count();
    assert_eq!(init_count, 3);
}

#[test]
fn programs_to_ll_multi_file_and_private_sibling() {
    // 入口调用兄弟命名空间函数 + 兄弟同名顶层函数（不误报 ambiguous）：联合检查 + 合并 codegen
    let entry = hc::parse_source(
        "fn load_config(x: i32) i32 { return x + 1; }\nfn main() i32 { return load_config(1) + Math.square(4); }\n",
    )
    .unwrap();
    let sib = hc::parse_source(
        "fn load_config(x: i32) i32 { return x * 2; }\nnamespace Math { fn square(x: i32) i32 { return x * x; } }\n",
    )
    .unwrap();
    let ll = programs_to_ll(
        &entry,
        "fn load_config(x: i32) i32 { return x + 1; }\nfn main() i32 { return load_config(1) + Math.square(4); }\n",
        &[&sib],
    )
    .expect("codegen");
    assert!(ll.contains("define"), "应生成函数定义");
    assert!(ll.contains("@main"), "应生成入口 wrapper");
}

#[test]
fn strip_test_funcs_remaps_index() {
    // 剔除 [test] fn 后：扁平/限定名保留且索引重映射到正确函数；[test] fn 名移除
    let mut m = hc::ir::lower(
        &hc::parse_source(
            "[test] fn a() !void {}\nfn helper() i32 { return 1; }\nnamespace N { fn f() i32 { return 2; } }\n",
        )
        .unwrap(),
    )
    .unwrap();
    strip_test_funcs_in_place(&mut m);
    assert!(m.funcs.iter().all(|f| !f.is_test));
    assert!(m.func_index.contains_key("helper"));
    assert!(m.func_index.contains_key("N.f"));
    assert!(!m.func_index.contains_key("a"));
    assert_eq!(m.funcs[m.func_index["helper"][0]].name, "helper");
    assert_eq!(m.funcs[m.func_index["N.f"][0]].name, "f");
}

#[test]
fn test_runner_runs_only_entry_tests() {
    // 兄弟文件 [test] fn 文件私有：测试跑器只调用入口的 test fn
    let entry_src = "[test] fn a() !void {}\nfn main() i32 { return 0; }\n";
    let entry = hc::parse_source(entry_src).unwrap();
    let sib = hc::parse_source("[test] fn b() !void {}\n").unwrap();
    let ll = programs_to_test_ll(&entry, entry_src, &[&sib]).expect("codegen_tests");
    assert!(ll.contains("[RUN] a"), "应含入口测试 a 的运行标记");
    assert!(!ll.contains("[RUN] b"), "不应含兄弟测试 b 的运行标记");
}

#[test]
fn color_helpers_paint_and_test_line() {
    // paint：开=true 产 ANSI 码，关=false 原样返回
    assert_eq!(paint(true, "32", "x"), "\u{1b}[32mx\u{1b}[0m");
    assert_eq!(paint(false, "32", "x"), "x");
    // color_test_line：终端下 [PASS]/[FAIL]/[SKIP] 分别绿/红/黄，其余原样
    assert_eq!(
        color_test_line("[PASS] a", true),
        "\u{1b}[32m[PASS]\u{1b}[0m a"
    );
    assert_eq!(
        color_test_line("[FAIL] b (error.X)", true),
        "\u{1b}[31m[FAIL]\u{1b}[0m b (error.X)"
    );
    assert_eq!(
        color_test_line("[SKIP] c", true),
        "\u{1b}[33m[SKIP]\u{1b}[0m c"
    );
    assert_eq!(color_test_line("[PASS] a", false), "[PASS] a");
    assert_eq!(color_test_line("other line", true), "other line");
}

#[test]
fn lint_unused_var_detected() {
    let src = "fn main() void { var x: i32 = 42; }";
    let program = hc::parse_source(src).unwrap();
    let diags = crate::lint::lint_source(src, &program, false);
    assert!(diags.iter().any(|d| d.rule.name == "unused_var"));
}

#[test]
fn lint_unused_var_skipped_underscore() {
    let src = "fn main() void { var _x: i32 = 42; }";
    let program = hc::parse_source(src).unwrap();
    let diags = crate::lint::lint_source(src, &program, false);
    assert!(!diags.iter().any(|d| d.rule.name == "unused_var"));
}

#[test]
fn lint_redundant_eq_false_detected() {
    let src = "fn main() bool { var x: bool = true; return x == false; }";
    let program = hc::parse_source(src).unwrap();
    let diags = crate::lint::lint_source(src, &program, false);
    assert!(diags.iter().any(|d| d.rule.name == "redundant_eq_false"));
}
