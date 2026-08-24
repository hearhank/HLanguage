//! LSP 语言服务器入口：main 函数

// B2：`hc-lsp` 独立二进制入口（`hc lsp` 子命令通过 hc-tools 调用 hc_lsp::run_server()）

fn main() {
    hc_lsp::run_server();
}
