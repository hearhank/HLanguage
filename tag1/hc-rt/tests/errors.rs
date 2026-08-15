//! 梯队 1 第二轮验收测试（M2.6 错误集检查 / M4.2 @panic / io.exit / ExitType）

use hc_rt::Interp;

/// 运行源码中所有 test fn；断言全部通过
fn run_ok(src: &str) {
    let program = hc::parse_source(src).unwrap_or_else(|d| panic!("parse: {:?}", d));
    let mut interp = Interp::new(src);
    interp
        .load(&program)
        .unwrap_or_else(|e| panic!("load: {} {}", e.name, e.message));
    let (p, f, _s) = interp.run_tests();
    assert_eq!(f, 0, "failed: {:?}", interp.test_out);
    assert!(p >= 1, "no tests ran");
}

/// 断言 load 阶段编译错误（语义检查拦截）
fn run_compile_error(src: &str, err_frag: &str) {
    let program = hc::parse_source(src).expect("parse should succeed");
    let mut interp = Interp::new(src);
    let e = interp
        .load(&program)
        .expect_err("semantic check should reject");
    assert!(
        e.message.contains(err_frag),
        "expected error containing `{err_frag}`, got: {} ({})",
        e.name,
        e.message
    );
}

#[test]
fn error_set_member_ok() {
    // return error.NotFound 属于 FileError → 通过
    run_ok(
        "const FileError = error{ NotFound, PermissionDenied };
fn read() FileError!i32 {
    return error.NotFound;
}
test fn t() !void {
    try expect_error(error.NotFound, read());
}
",
    );
}

#[test]
fn error_set_member_rejected() {
    // return error.Other 不属于 FileError → 编译错误（M2.6）
    run_compile_error(
        "const FileError = error{ NotFound };
fn read() FileError!i32 {
    return error.Other;
}
fn main(io: Io) !void {}
",
        "not in declared error set",
    );
}

#[test]
fn anyerror_not_checked() {
    // anyerror!T：不检查（契约不约束具体错误集）
    run_ok(
        "fn read() anyerror!i32 {
    return error.AnyThing;
}
test fn t() !void {
    try expect_error(error.AnyThing, read());
}
",
    );
}

#[test]
fn panic_aborts() {
    // @panic：运行期 abort（带消息）
    let src = "fn main(io: Io) !void { @panic(\"boom\"); }\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    let e = interp.run_main().expect_err("panic should abort");
    assert_eq!(e.name, "Panic");
    assert!(e.message.contains("boom"));
}

#[test]
fn io_exit_success() {
    // io.exit(ExitType.Exit, 0)：正常退出
    let src = "fn main(io: Io) !void { io.exit(ExitType.Exit, 0); }\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    interp.run_main().expect("exit should be normal");
    assert_eq!(interp.exit_code, Some(0));
}

#[test]
fn io_exit_error_code() {
    // io.exit(ExitType.Error, 3)：错误退出码
    let src = "fn main(io: Io) !void { io.exit(ExitType.Error, 3); }\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    interp.run_main().expect("exit should be captured");
    assert_eq!(interp.exit_code, Some(3));
}

#[test]
fn exit_type_variants() {
    // ExitType 内建枚举：Exit / Error 两个变体
    run_ok(
        "test fn t() !void {
    try expect_eq(@intFromEnum(ExitType.Exit), 0);
    try expect_eq(@intFromEnum(ExitType.Error), 1);
}\n",
    );
}

#[test]
fn root_error_reports_location() {
    // M2.6：未处理错误到达根作用域 → 记录错误名位置（原始错误定位，不输出调用链）后 panic 式中止
    let src = "const FileError = error{ NotFound };\nfn f() FileError!i32 {\n    return error.NotFound;\n}\nfn main(io: Io) !void {\n    var v = try f();\n}\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    let e = interp.run_main().expect_err("未处理错误应传播到根");
    assert_eq!(e.name, "NotFound");
    // 位置 = 错误名首次出现处（错误集声明第 1 行）
    let sp = e.span.expect("根作用域错误应带位置");
    assert_eq!(sp.line, 1);
}

#[test]
fn root_error_location_prefers_decl() {
    // 位置优先指向错误名首次出现（此处为 return 字面量行）
    let src = "fn f() anyerror!i32 {\n    return error.NotFound;\n}\nfn main(io: Io) !void {\n    var v = try f();\n}\n";
    let program = hc::parse_source(src).expect("parse");
    let mut interp = Interp::new(src);
    interp.load(&program).expect("load");
    let e = interp.run_main().expect_err("未处理错误应传播到根");
    let sp = e.span.expect("带位置");
    assert_eq!(sp.line, 2, "首次出现 = return error.NotFound 所在行");
}

#[test]
fn unmarked_error_return_rejected() {
    // 未标记错误类型：函数返回类型非错误联合 → return error.X 编译错误
    // （错误不进入传播链；运行时由根作用域兜底 panic）
    run_compile_error(
        "fn f() i32 { return error.NotFound; }\nfn main(io: Io) !void {}\n",
        "does not declare",
    );
}

#[test]
fn unmarked_error_return_rejected_void() {
    // 无返回类型（隐式 void）同样拒绝
    run_compile_error(
        "fn f() void { return error.NotFound; }\nfn main(io: Io) !void {}\n",
        "does not declare",
    );
}

#[test]
fn marked_error_propagates_to_catch() {
    // 标记错误联合：错误沿调用链传播（try/裸传递），直到 catch 处理
    run_ok(
        "const FileError = error{ NotFound };\nfn level3() FileError!i32 { return error.NotFound; }\nfn level2() FileError!i32 { var x = try level3(); return x; }\nfn level1() FileError!i32 { return level2(); }\ntest fn t() !void {\n    var v = level1() catch 42;\n    try expect_eq(v, 42);\n}\n",
    );
}

#[test]
fn marked_error_catch_binds_name() {
    // catch |err| 捕获错误名（沿链传播的原始错误）
    run_ok(
        "const FileError = error{ NotFound };\nfn a() FileError!i32 { return error.NotFound; }\nfn b() FileError!i32 { return a(); }\nfn c() FileError!i32 { var x = try b(); return x; }\ntest fn t() !void {\n    var got: i32 = 0;\n    var v = c() catch |err| {\n        try expect_eq(err, error.NotFound);\n        got = 1;\n        0;\n    };\n    try expect_eq(got, 1);\n}\n",
    );
}

#[test]
fn marked_error_payload_ok_path() {
    // 成功路径：错误联合函数返回 payload 不被 catch 拦截
    run_ok(
        "const FileError = error{ NotFound };\nfn f(x: i32) FileError!i32 {\n    if (x < 0) return error.NotFound;\n    return x;\n}\ntest fn t() !void {\n    var v = try f(5);\n    try expect_eq(v, 5);\n}\n",
    );
}
