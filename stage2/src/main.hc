// stage2/src/main.hc — H 编译器（H 实现）：入口 + 阶段调度
// S1/S2：读目标源文件 → lex（src/lexer.hc，同命名空间扁平共享，ADR-0031）→ parse（S3 填充）。
// 纪律自查清单见 stage2/README.md；运行方式：
//   包模式：hc run stage2 stage2/test/smoke.hc（Rust 包加载）
//   检查：hc run stage1/checker.hc stage2/src/main.hc
//   链路：hc run stage1/interp.hc stage2/src/main.hc <target.hc>
//   对照：hc run stage1/interp.hc stage2/src/main.hc --dump-tokens <target.hc>（= hc lex 格式）
// 阶段 2：语法分析（S3 填充真实 Parser；当前为骨架占位）
// ============================================================
// 阶段 2：语法分析（S3：src/parser.hc）
// ============================================================

fn parse_tokens(toks: Vec<Token>) AstNode {
    var p: Parser = alloc.init(Parser{ tokens = toks, pos = 0, n = toks.len, rev_kw_map = build_rev_kw_map() });
    return p.parse_program();
}

// ============================================================
// 入口
// ============================================================

fn main(args: Vec<String>) !void {
    // S3：AST 转储（与 hc run stage1/interp.hc --dump-ast 同格式）
    if (args.len >= 3 and args[1].as_slice() == "--dump-ast") {
        var dsrc = try io.fs.read_file(args[2], alloc);
        var dast = parse_tokens(lex_source(dsrc));
        var dumper: AstDumper = alloc.init(AstDumper{ buf = Vec<u8>.init(alloc) });
        dumper.dump(dast, 0);
        io.print("{}", dumper.buf.as_slice());
        return;
    }
    if (args.len < 2) {
        io.print("usage: main [--dump-tokens] [--dump-ast] <source.hc>\n");
        return error.Usage;
    }
    var path = args[1];
    // 阶段 0：读源文件（宿主透传；缺失 → err 上浮，main 非零退出）
    var src = try io.fs.read_file(path, alloc);
    // 阶段 1：词法（S2 已落地）
    var toks = lex_source(src);
    // 阶段 2：语法（S3 已落地）
    var ast = parse_tokens(toks);
    // 阶段 3：语义检查（S4：src/checker.hc，同命名空间扁平共享）
    var checker: Checker = alloc.init(Checker{
        diags = Vec<Vec<u8>>.init(alloc),
        src = Vec<u8>.init(alloc),
        line_starts = Vec<usize>.init(alloc),
        scopes = Vec<ScopeEntry>.init(alloc),
        scope_sizes = Vec<usize>.init(alloc),
        types = Map<&[u8], SType>.init(alloc),
        funcs = Map<&[u8], FnSig>.init(alloc),
        current_fn_ret_is_error_union = false,
        current_class = "",
    });
    checker.init(src);
    checker.check_program(ast);
    if (checker.diags.len > 0) {
        checker.report();
        return error.CheckFailed;
    }
    // 阶段 4+：lower / HBC2 编码（S6–S7 填充）
    io.print("stage2: {} bytes -> {} tokens -> {} decls, check ok\n", src.len, toks.len, ast.children.len);
}
