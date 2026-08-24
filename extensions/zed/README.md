# H Language Extension for Zed

This extension provides H language support for the Zed editor, including:

- **Syntax highlighting** via Tree-sitter
- **Code diagnostics** via LSP (Language Server Protocol)
- **Go-to-definition** via LSP
- **Hover information** via LSP
- **Auto-completion** via LSP
- **Code formatting** via `hc fmt`

## Installation

### One-Command Deploy (Windows)

From a `cmd` prompt, run the deployment script. It builds the LSP server,
generates the Tree-sitter parser, builds the Rust extension, and stages
everything:

```bat
cd tag1\hc-lsp
deploy-lsp.bat
```

The script:
- Builds `hc-lsp` and copies it (plus `hc`) to `bin\`
- Generates the Tree-sitter parser into `extensions\zed\languages\h\src`
- Builds the Rust extension (`extension.wasm`)
- Stages the extension and writes a Zed settings snippet to `zed-lsp-snippet.json`

### Install in Zed

1. **Add `bin` to PATH** so Zed can find `hc-lsp`: run `setup-lsp.bat` (or
   manually add `%PROJECT_ROOT%\bin` to your user PATH).
2. **Install the dev extension** (this is how Zed actually loads the extension):
   - Open Zed, run `zed: install dev extension` from the command palette
   - Select the `extensions\zed` directory
   - Zed compiles the Rust extension and the Tree-sitter grammar automatically
     (it downloads `wasm32-wasip2` + wasi-sdk on first use).
3. If the LSP does not start (e.g. wasm compile failed), merge the generated
   `zed-lsp-snippet.json` into `%APPDATA%\Zed\settings.json`:
   ```json
   {
     "lsp": {
       "hc-lsp": {
         "binary": { "path": "C:\\path\\to\\bin\\hc-lsp.exe" }
       }
     },
     "languages": {
       "H": { "language_servers": ["hc-lsp", "!language-server"] }
     }
   }
   ```
4. **Restart Zed** and open a `.hc` file.

> The grammar manifest points at this repository itself
> (`repository = "file:///C:/Users/Hank/Documents/works/AI/H2"`, `rev` =
> `feature/improv_code_v0.1.5`, `path = "extensions/zed/languages/h"`). Zed
> clones the repo shallowly and compiles `src/parser.c`. Because it is a
> shallow clone of this repo, **commit** `src/parser.c` and any grammar/query
> changes before re-installing the dev extension.

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
3. Make sure the file has a `.hc` extension

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
