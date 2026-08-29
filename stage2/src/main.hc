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
    // S6/S7：--emit-hbc <out.hbc> <in.hc> [in2.hc...]——多文件合并编译（单 Program，声明按文件序合并）
    if (args.len >= 3 and args[1].as_slice() == "--emit-hbc") {
        if (args.len < 4) {
            io.print("usage: main --emit-hbc <out.hbc> <in.hc> [more.hc...]\n");
            return error.Usage;
        }
        var out_path = args[2];
        var prog = make_node("Program");
        var mut fi: usize = 3;
        while (fi < args.len) {
            var fsrc = try io.fs.read_file(args[fi], alloc);
            try io.fs.write_file("stage2/test/progress.txt", "read ok", alloc);
            var ftoks = lex_source(fsrc);
            try io.fs.write_file("stage2/test/progress.txt", "lex ok", alloc);
            var fast = parse_tokens(ftoks);
            try io.fs.write_file("stage2/test/progress.txt", "parse ok", alloc);
            var mut di: usize = 0;
            while (di < fast.children.len) {
                node_add_child(&prog, fast.children[di]);
                di += 1;
            }
            fi += 1;
            // S8 进度标记（interp 全链数小时无 stdout，落盘盯进度）
            var mut mk = Vec<u8>.init(alloc);
            append_bytes(&mk, "file ");
            append_int(fi - 3, &mut mk);
            try io.fs.write_file("stage2/test/progress.txt", mk.as_slice(), alloc);
        }
        // 语义检查（合并后单 Program；src 取首文件仅作诊断定位）
        var src0 = try io.fs.read_file(args[3], alloc);
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
        checker.init(src0);
        checker.check_program(prog);
        try io.fs.write_file("stage2/test/progress.txt", "check ok", alloc);
        if (checker.diags.len > 0) {
            checker.report();
            return error.CheckFailed;
        }
        // lower（S6）：子集外构造响亮失败
        var l = lower_module(prog);
        try io.fs.write_file("stage2/test/progress.txt", "lower ok", alloc);
        if (l.errs.len > 0) {
            io.print("lower failed: {} diagnostics\n", l.errs.len);
            var mut ei: usize = 0;
            while (ei < l.errs.len) {
                io.print("  - {}\n", l.errs[ei]);
                ei += 1;
            }
            return error.LowerFailed;
        }
        // HBC2 编码（S7）落盘
        var m = lower_finish(&l);
        var bytes = enc_module(m);
        try io.fs.write_file(out_path, bytes.as_slice(), alloc);
        try io.fs.write_file("stage2/test/progress.txt", "encode ok", alloc);
        io.print("stage2: {} files -> {} decls -> {} funcs -> {} bytes -> {}\n", args.len - 3, prog.children.len, l.funcs.len, bytes.len, out_path);
        return;
    }
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
