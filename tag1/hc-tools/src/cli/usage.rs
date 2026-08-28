//! `hc --help` 使用说明文本

pub(crate) const USAGE: &str = "hc <command> [args...]

H 语言工具链（tag1 垂直切片）

USAGE:
    hc run <file.hc> [--dangle=on|off|auto]
                              运行脚本模式（解释执行；--dangle 控制悬垂指针检查）
    hc run <file.hbc> [--dangle=on|off|auto]
                              运行字节码 VM（M3.2，装载 HBC2；全语言，同 IR）
    hc run --ir <file.hc>      用 IR 参考解释器运行（全语言，interp == IR）
    hc test [--mode=interpret|compile] [--dangle=on|off|auto] [file.hc|dir]
                              运行 test fn（默认当前目录全部 .hc；--mode=compile 原生交叉验证）
    hc check <file.hc>         仅检查（词法/语法/装载）
    hc errors <file.hc>        输出错误码表（M2.6：错误名 ↔ 码 + 位置）
    hc build <file.hc>         编译为原生可执行（LLVM IR + zig cc）
    hc init <name>             创建新项目骨架（build.zon + main.hc，组 H1）
    hc pkg add <name> [--path <dir>] [--version <ver>]
                              写本地依赖声明到 build.zon deps（组 H2）
    hc pkg publish          从当前目录发布包到本地注册中心（~/.hc/registry/，B3）
    hc doc [target] [--out <dir>]
                              生成 Markdown 文档（/// 注释 + 声明签名；target 默认当前目录包，
                              `std` = 标准库内置目录页；输出默认 <target 目录>/docs/api/，组 H4）
    hc lint <file.hc|dir> [--json] [--fix]
                              静态诊断（命名规范补全——缩写全大写、未用变量、可简化构造；
                              6 条规则 L001–L006；--json 输出 JSON，--fix 自动修复 4 规则）
    hc fmt <file.hc|dir> [--check]
                              格式化 .hc 源码（token 级重排，AST 保真；默认原地写回，
                              --check 仅报告将改动的文件，组 I1）
    hc lex <file.hc>          转储 token 流（K1 对照：`{start} {end} {line} {col} {kind:?}`，
                              与 H 版 lexer 输出逐行 diff）
    hc cc <file.c> [--output <file>]
                              编译 C 文件（zig cc 封装，产出原生目标文件或可执行）
    hc lsp                    启动 LSP 语言服务器（stdio 通道，供编辑器集成）
    hc --version
    hc --help
";
