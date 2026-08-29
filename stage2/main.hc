// stage2/main.hc — H 编译器（H 实现）入口：阶段调度
// S1 骨架：读目标源文件 → lex → parse（S2/S3 起逐段填充真实实现）。
// 纪律自查清单见 stage2/README.md；运行方式：
//   检查：hc run stage1/checker.hc stage2/main.hc
//   运行：hc run stage1/interp.hc stage2/main.hc <target.hc>
import .{lexer};
import .{parser};

fn main(args: Vec<String>) !void {
    if (args.len < 2) {
        io.print("usage: main <source.hc>\n");
        return error.Usage;
    }
    var path = args[1];
    // 阶段 0：读源文件（宿主透传；缺失 → err 上浮，main 非零退出）
    var src = try io.fs.read_file(path, alloc);
    // 阶段 1：词法（S2 填充真实 Lexer）
    var ntoks = lex_source(src);
    // 阶段 2：语法（S3 填充真实 Parser）
    var nnodes = parse_tokens(ntoks);
    // 阶段 3+：语义检查 / lower / HBC2 编码（S4–S7 填充）
    io.print("stage2 skeleton: {} bytes -> {} tokens -> {} nodes\n", src.len, ntoks, nnodes);
}
