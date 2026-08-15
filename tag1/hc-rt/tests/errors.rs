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
}
",
    );
}
