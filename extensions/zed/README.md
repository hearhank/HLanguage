# H Language Extension for Zed

This extension provides H language support for the Zed editor, including:

- **Syntax highlighting** via Tree-sitter
- **Code diagnostics** via LSP (Language Server Protocol)
- **Go-to-definition** via LSP
- **Hover information** via LSP
- **Auto-completion** via LSP
- **Code formatting** via `hc fmt`

## Installation

### Prerequisites

1. **Install H language compiler** (`hc`):
   ```bash
   # Clone the H language repository
   git clone https://github.com/h-language/h
   cd h
   
   # Build and install
   cargo build --release
   cargo install --path tag1/hc
   ```

2. **Install Tree-sitter CLI** (for syntax highlighting):
   ```bash
   npm install -g tree-sitter-cli
   ```

### Install the Extension

1. **Clone the extension**:
   ```bash
   git clone https://github.com/h-language/h
   cd h/extensions/zed
   ```

2. **Generate Tree-sitter parser**:
   ```bash
   cd languages/h
   tree-sitter generate
   ```

3. **Install in Zed**:
   - Open Zed
   - Press `Cmd+,` (macOS) or `Ctrl+,` (Linux/Windows) to open settings
   - Add the extension path to `extensions`:
     ```json
     {
       "extensions": {
         "h-language": "/path/to/h/extensions/zed"
       }
     }
     ```

4. **Restart Zed**

## Configuration

### LSP Server

The extension uses `hc-lsp` as the language server. Make sure `hc-lsp` is in your PATH:

```bash
# Build hc-lsp
cd tag1/hc-lsp
cargo build --release

# Add to PATH
export PATH=$PATH:/path/to/h/tag1/target/release
```

### Formatter

The extension uses `hc fmt` for code formatting. Make sure `hc` is in your PATH.

## Features

### Diagnostics

The LSP server provides real-time diagnostics for:
- Syntax errors
- Type errors
- Semantic errors

### Go-to-Definition

Jump to the definition of:
- Functions
- Classes
- Enums
- Interfaces
- Variables
- Constants
- Namespaces
- Fields

### Hover Information

Hover over a symbol to see:
- Symbol name
- Symbol type (function, class, enum, etc.)
- Definition location (file:line:column)

### Auto-Completion

Get completions for:
- Keywords (fn, var, const, if, else, while, for, etc.)
- Types (i32, f64, bool, String, Vec, etc.)
- Functions
- Variables
- Fields

### Code Formatting

Format code using `hc fmt`:
- Press `Shift+Alt+F` (or `Cmd+Shift+F` on macOS)
- Or enable "format on save" in Zed settings

## Development

### Build LSP Server

```bash
cd tag1/hc-lsp
cargo build --release
```

### Run Tests

```bash
cd tag1/hc-lsp
cargo test
```

### Generate Tree-sitter Parser

```bash
cd extensions/zed/languages/h
tree-sitter generate
tree-sitter test
```

## Troubleshooting

### LSP Server Not Found

If Zed can't find `hc-lsp`:
1. Make sure `hc-lsp` is built: `cargo build --release`
2. Add the binary to your PATH
3. Restart Zed

### Syntax Highlighting Not Working

If syntax highlighting doesn't work:
1. Make sure Tree-sitter CLI is installed: `npm install -g tree-sitter-cli`
2. Generate the parser: `cd extensions/zed/languages/h && tree-sitter generate`
3. Restart Zed

### Diagnostics Not Showing

If diagnostics don't show:
1. Check if `hc-lsp` is running: `ps aux | grep hc-lsp`
2. Check Zed logs: `View -> Toggle Log Viewer`
3. Make sure the file has a `.h` extension

## Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test --workspace`
5. Submit a pull request

## License

MIT License - see LICENSE file for details.

## Links

- [H Language Repository](https://github.com/h-language/h)
- [Zed Editor](https://zed.dev)
- [Tree-sitter](https://tree-sitter.github.io/tree-sitter/)
- [LSP Specification](https://microsoft.github.io/language-server-protocol/)
