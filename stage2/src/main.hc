// stage2/src/main.hc — H 编译器（H 实现）：入口 + 阶段调度
// S1/S2：读目标源文件 → lex（src/lexer.hc，同命名空间扁平共享，ADR-0031）→ parse（S3 填充）。
// 纪律自查清单见 stage2/README.md；运行方式：
//   包模式：hc run stage2 stage2/test/smoke.hc（Rust 包加载）
//   检查：hc run stage1/checker.hc stage2/src/main.hc
//   链路：hc run stage1/interp.hc stage2/src/main.hc <target.hc>
//   对照：hc run stage1/interp.hc stage2/src/main.hc --dump-tokens <target.hc>（= hc lex 格式）
// 阶段 2：语法分析（S3 填充真实 Parser；当前为骨架占位）

// ============================================================
// 阶段 2：语法分析（S3 填充真实 Parser；当前为骨架占位）
// ============================================================

fn parse_tokens(ntoks: usize) usize {
    return 0;
}

// ============================================================
// 入口
// ============================================================

fn main(args: Vec<String>) !void {
    // S2：token 流转储（K1 对照模式）
    if (args.len >= 3 and args[1].as_slice() == "--dump-tokens") {
        var dsrc = try io.fs.read_file(args[2], alloc);
        dump_tokens(lex_source(dsrc));
        return;
    }
    if (args.len < 2) {
        io.print("usage: main [--dump-tokens] <source.hc>\n");
        return error.Usage;
    }
    var path = args[1];
    // 阶段 0：读源文件（宿主透传；缺失 → err 上浮，main 非零退出）
    var src = try io.fs.read_file(path, alloc);
    // 阶段 1：词法（S2 已落地）
    var toks = lex_source(src);
    // 阶段 2：语法（S3 填充真实 Parser）
    var nnodes = parse_tokens(toks.len);
    // 阶段 3+：语义检查 / lower / HBC2 编码（S4–S7 填充）
    io.print("stage2: {} bytes -> {} tokens -> {} nodes\n", src.len, toks.len, nnodes);
}
