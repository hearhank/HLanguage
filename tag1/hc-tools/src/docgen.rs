//! H4/H5：`hc doc`——Markdown 文档生成。
//!
//! 形态：`hc doc [target] [--out <dir>]`；target = 文件 / 目录（包）/ `std`。
//!
//! - 项目页：扫描 `///` 文档注释（lexer 跳过，故从原始源码提取）+ AST 声明签名，
//!   生成每文件一页 Markdown + 包索引页。
//! - 标准库页：H.std 为 Rust 内建（无 .hc 源），由内置目录化摘要生成。
//! - 输出目录约定：`<target 所在目录>/docs/api/`（`--out` 覆盖）。

use std::path::{Path, PathBuf};

use hc::ast::{Decl, Method, Param, Type};
use hc::parse_source;
use hc::token::Span;

use crate::buildzon;

// ---------- `///` 文档注释提取（从原始源码） ----------

/// 连续 `///` 行构成一个文档块；`end` = 块末行之后的字节偏移。
pub struct DocRun {
    pub end: usize,
    pub text: String,
}

fn collect_doc_runs(src: &str) -> Vec<DocRun> {
    let mut runs = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut cur_end = 0usize;
    let mut pos = 0usize;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        pos += line.len();
        if let Some(rest) = t.strip_prefix("///") {
            cur.push(rest.trim_start_matches(' ').to_string());
            cur_end = pos;
        } else if !cur.is_empty() {
            runs.push(DocRun {
                end: cur_end,
                text: std::mem::take(&mut cur).join("\n"),
            });
        }
    }
    if !cur.is_empty() {
        runs.push(DocRun {
            end: cur_end,
            text: cur.join("\n"),
        });
    }
    runs
}

/// doc 与声明之间的间隙是否为「特性前缀」：仅空白 + `pub` + `[...]` 标注
/// （`[test]`/`[continuous]`/`[module]` 等）。parser 的 span.start 落在 fn/class
/// 关键字，`pub` 与标注在 span 之外——间隙须允许这些前缀才能关联 doc。
fn gap_is_doc_prefix(gap: &str) -> bool {
    let b = gap.as_bytes();
    let mut pos = 0usize;
    loop {
        while pos < b.len() && (b[pos] as char).is_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return true;
        }
        if gap[pos..].starts_with("pub")
            && b.get(pos + 3)
                .map_or(true, |&c| !(c as char).is_alphanumeric() && c != b'_')
        {
            pos += 3;
            continue;
        }
        if b[pos] == b'[' {
            let mut depth = 1usize;
            pos += 1;
            while pos < b.len() && depth > 0 {
                match b[pos] {
                    b'[' => depth += 1,
                    b']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                pos += 1;
            }
            if depth != 0 {
                return false;
            }
            continue;
        }
        return false;
    }
}

/// 取出恰在 `decl_start` 之前的文档块（间隙为特性前缀）；无则 None。
fn doc_before(src: &str, runs: &mut Vec<DocRun>, decl_start: usize) -> Option<String> {
    let mut best: Option<usize> = None;
    for (i, r) in runs.iter().enumerate() {
        if r.end <= decl_start && gap_is_doc_prefix(&src[r.end..decl_start]) {
            if best.map_or(true, |b| runs[b].end < r.end) {
                best = Some(i);
            }
        }
    }
    best.map(|i| runs.remove(i).text)
}

// ---------- 签名渲染（AST 重构） ----------

pub fn render_type(t: &Type) -> String {
    match t {
        Type::Named(n, args) if args.is_empty() => n.clone(),
        Type::Named(n, args) => format!(
            "{n}<{a}>",
            a = args.iter().map(render_type).collect::<Vec<_>>().join(", ")
        ),
        Type::Ptr(inner, mut_) => format!(
            "*{} {}",
            if *mut_ { "mut " } else { "" },
            render_type(inner)
        ),
        Type::Slice(inner, mut_) => format!(
            "&{}[{}]",
            if *mut_ { "mut " } else { "" },
            render_type(inner)
        ),
        Type::Optional(inner) => format!("?{}", render_type(inner)),
        Type::ErrorUnion(Some(e), t) => format!("{}!{}", render_type(e), render_type(t)),
        Type::ErrorUnion(None, t) => format!("!{}", render_type(t)),
        Type::Tuple(ts) => format!(
            "({})",
            ts.iter().map(render_type).collect::<Vec<_>>().join(", ")
        ),
        Type::Array(n, t) => format!("[{n}]{}", render_type(t)),
        Type::ComptimeInt(v) => format!("{v}"),
        Type::Infer => "_".to_string(),
        Type::Owned(inner) => format!("o {}", render_type(inner)),
    }
}

fn render_param(p: &Param) -> String {
    let mut s = format!("{}: {}", p.name, render_type(&p.ty));
    if p.default.is_some() {
        s.push_str(" = ...");
    }
    s
}

fn render_method_sig(m: &Method) -> String {
    let mut s = format!("fn {}({})", m.name, {
        let mut ps = Vec::new();
        for p in &m.params {
            ps.push(render_param(p));
        }
        ps.join(", ")
    });
    if let Some(r) = &m.ret {
        s.push(' ');
        s.push_str(&render_type(r));
    }
    s
}

fn render_fn_sig(
    name: &str,
    params: &[Param],
    ret: &Option<Type>,
    is_test: bool,
    test_name: &Option<String>,
    pub_: bool,
    exported: bool,
) -> String {
    let mut s = String::new();
    if pub_ {
        s.push_str("pub ");
    }
    if exported {
        s.push_str("export ");
    }
    if is_test {
        match test_name {
            Some(n) => s.push_str(&format!("[test({n:?})] ")),
            None => s.push_str("[test] "),
        }
    }
    s.push_str(&format!("fn {name}("));
    s.push_str(
        &params
            .iter()
            .map(render_param)
            .collect::<Vec<_>>()
            .join(", "),
    );
    s.push(')');
    if let Some(r) = ret {
        s.push(' ');
        s.push_str(&render_type(r));
    }
    s
}

fn decl_span(d: &Decl) -> &Span {
    match d {
        Decl::Global { span, .. }
        | Decl::Const { span, .. }
        | Decl::Fn { span, .. }
        | Decl::Class { span, .. }
        | Decl::Enum { span, .. }
        | Decl::Union { span, .. }
        | Decl::Interface { span, .. }
        | Decl::Namespace { span, .. }
        | Decl::Using { span, .. }
        | Decl::Import { span, .. }
        | Decl::Script { span, .. }
        | Decl::Comptime { span, .. } => span,
    }
}

/// 声明标题锚（`fn main` / `[test] fn t` / `class Line` / `[module] namespace Orders`）。
fn decl_anchor(d: &Decl) -> String {
    match d {
        Decl::Fn {
            name,
            is_test,
            test_name,
            ..
        } => {
            let mut s = String::new();
            if *is_test {
                match test_name {
                    Some(n) => s.push_str(&format!("[test({n:?})] ")),
                    None => s.push_str("[test] "),
                }
            }
            s.push_str(&format!("fn {name}"));
            s
        }
        Decl::Class { name, .. } => format!("class {name}"),
        Decl::Enum { name, .. } => format!("enum {name}"),
        Decl::Union { name, .. } => format!("union {name}"),
        Decl::Interface { name, .. } => format!("interface {name}"),
        Decl::Namespace {
            name, is_module, ..
        } => {
            if *is_module {
                format!("[module] namespace {name}")
            } else {
                format!("namespace {name}")
            }
        }
        Decl::Const { name, .. } => format!("const {name}"),
        Decl::Global { name, .. } => format!("global {name}"),
        Decl::Import { .. } | Decl::Using { .. } | Decl::Script { .. } | Decl::Comptime { .. } => {
            String::new()
        }
    }
}

// ---------- 页面渲染 ----------

fn render_import(d: &Decl) -> String {
    if let Decl::Import {
        path,
        alias,
        select,
        ..
    } = d
    {
        let path_s = path.join(".");
        match select {
            Some(syms) => {
                let sels = syms
                    .iter()
                    .map(|(s, a)| match a {
                        Some(ali) => format!("{s} as {ali}"),
                        None => s.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("import {path_s}.{{{sels}}};")
            }
            None => match alias {
                Some(a) => format!("import {path_s} as {a};"),
                None => format!("import {path_s};"),
            },
        }
    } else {
        String::new()
    }
}

/// 递归渲染一个声明（`level` 控制标题层级：0 → `###`，1 → `####`，…）。
fn render_decl(d: &Decl, src: &str, runs: &mut Vec<DocRun>, out: &mut String, level: usize) {
    let h = "#".repeat(3 + level);
    let doc = doc_before(src, runs, decl_span(d).start);
    match d {
        Decl::Import { .. } | Decl::Using { .. } => return, // 导入集中列出，不在此渲染
        Decl::Script { .. } => {
            if let Some(doc) = doc {
                out.push_str(&format!("{h} `script`\n\n{doc}\n\n"));
            }
        }
        Decl::Comptime { .. } => {
            if let Some(doc) = doc {
                out.push_str(&format!("{h} `comptime`\n\n{doc}\n\n"));
            }
        }
        Decl::Fn {
            name,
            params,
            ret,
            is_test,
            test_name,
            pub_,
            exported,
            ..
        } => {
            let sig = render_fn_sig(name, params, ret, *is_test, test_name, *pub_, *exported);
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            out.push('\n');
        }
        Decl::Const { name, ty, pub_, .. } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str(&format!("const {name}"));
            if let Some(t) = ty {
                sig.push_str(&format!(": {}", render_type(t)));
            }
            out.push_str(&format!(
                "{h} `{}`\n```hc\n{sig} = …\n```\n",
                decl_anchor(d)
            ));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            out.push('\n');
        }
        Decl::Global { name, ty, pub_, .. } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str("global ");
            sig.push_str(name);
            if let Some(t) = ty {
                sig.push_str(&format!(": {}", render_type(t)));
            }
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            out.push('\n');
        }
        Decl::Class {
            name,
            ifaces,
            traits,
            fields,
            methods,
            pub_,
            ..
        } => {
            let mut sig = String::new();
            for t in traits {
                sig.push_str(&format!("{t:?} "));
            }
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str(&format!("class {name}"));
            if !ifaces.is_empty() {
                sig.push_str(&format!(
                    ": {}",
                    ifaces
                        .iter()
                        .map(render_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            sig.push_str(" { ");
            sig.push_str(
                &fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, render_type(&f.ty)))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if !methods.is_empty() {
                sig.push_str(" … }");
            } else {
                sig.push_str(" }");
            }
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            if !methods.is_empty() {
                out.push_str(&format!("{h} 方法\n\n"));
                for m in methods {
                    let md = doc_before(src, runs, m.span.start);
                    out.push_str(&format!(
                        "- `{}`{}",
                        render_method_sig(m),
                        if let Some(md) = md {
                            format!(" — {md}")
                        } else {
                            String::new()
                        }
                    ));
                    out.push('\n');
                }
                out.push('\n');
            }
        }
        Decl::Enum {
            name,
            variants,
            pub_,
            ..
        } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str(&format!("enum {name} {{ "));
            sig.push_str(
                &variants
                    .iter()
                    .map(|v| match &v.payload {
                        Some(t) => format!("{}({})", v.name, render_type(t)),
                        None => v.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            sig.push_str(" }");
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            out.push('\n');
        }
        Decl::Union {
            name, fields, pub_, ..
        } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str(&format!("union {name} {{ "));
            sig.push_str(
                &fields
                    .iter()
                    .map(|f| format!("{}: {}", f.name, render_type(&f.ty)))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            sig.push_str(" }");
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            out.push('\n');
        }
        Decl::Interface {
            name,
            supers,
            methods,
            pub_,
            ..
        } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            sig.push_str(&format!("interface {name}"));
            if !supers.is_empty() {
                sig.push_str(&format!(
                    ": {}",
                    supers
                        .iter()
                        .map(render_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            sig.push_str(" { … }");
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            if !methods.is_empty() {
                out.push_str(&format!("{h} 方法\n\n"));
                for m in methods {
                    let md = doc_before(src, runs, m.span.start);
                    out.push_str(&format!(
                        "- `{}`{}",
                        render_method_sig(m),
                        if let Some(md) = md {
                            format!(" — {md}")
                        } else {
                            String::new()
                        }
                    ));
                    out.push('\n');
                }
                out.push('\n');
            }
        }
        Decl::Namespace {
            name,
            decls,
            is_module,
            pub_,
            ..
        } => {
            let mut sig = String::new();
            if *pub_ {
                sig.push_str("pub ");
            }
            if *is_module {
                sig.push_str("[module] ");
            }
            sig.push_str(&format!("namespace {name} {{ … }}"));
            out.push_str(&format!("{h} `{}`\n```hc\n{sig}\n```\n", decl_anchor(d)));
            if let Some(doc) = doc {
                out.push_str(&format!("\n{doc}\n"));
            }
            if !decls.is_empty() {
                out.push('\n');
                for inner in decls {
                    render_decl(inner, src, runs, out, level + 1);
                }
            }
        }
    }
}

/// 渲染一个文件页（标题 = 文件相对路径去扩展名）。
pub fn render_file_page(rel_path: &str, src: &str) -> Result<String, String> {
    let program = parse_source(src).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mut runs = collect_doc_runs(src);
    let stem = rel_path.trim_end_matches(".hc").replace(['\\', '/'], ".");
    let mut out = String::new();
    out.push_str(&format!("# `{stem}`\n\n"));

    // 文件级文档：首个文档块在首条 import/using 之前（直接贴在声明上的 doc 归属该声明）
    if let Some(first) = program.decls.first() {
        let first_start = decl_span(first).start;
        let first_is_import = matches!(first, Decl::Import { .. } | Decl::Using { .. });
        if first_is_import
            && !runs.is_empty()
            && runs[0].end <= first_start
            && gap_is_doc_prefix(&src[runs[0].end..first_start])
        {
            let fd = runs.remove(0).text;
            out.push_str(&fd);
            out.push_str("\n\n");
        }
    }

    // 导入
    let imports: Vec<&Decl> = program
        .decls
        .iter()
        .filter(|d| matches!(d, Decl::Import { .. }))
        .collect();
    if !imports.is_empty() {
        out.push_str("## 导入\n\n");
        for imp in imports {
            out.push_str(&format!("- `{}`\n", render_import(imp)));
        }
        out.push('\n');
    }

    out.push_str("## 声明\n\n");
    let mut any = false;
    for d in &program.decls {
        if matches!(d, Decl::Import { .. } | Decl::Using { .. }) {
            continue;
        }
        render_decl(d, src, &mut runs, &mut out, 0);
        any = true;
    }
    if !any {
        out.push_str("（无声明）\n");
    }
    Ok(out)
}

// ---------- 生成入口 ----------

fn dir_hc_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("hc") && p.is_file() {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

fn write_page(out_dir: &Path, file_name: &str, text: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("创建 {} 失败: {e}", out_dir.display()))?;
    let path = out_dir.join(file_name);
    std::fs::write(&path, text).map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
    Ok(path)
}

/// 生成单个文件的文档页 → `out_dir/<stem>.md`。
pub fn generate_file(path: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let src =
        std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let rel = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled.hc");
    let page = render_file_page(rel, &src)?;
    let stem = rel.trim_end_matches(".hc");
    write_page(out_dir, &format!("{stem}.md"), &page)
}

/// 生成包（目录）文档：每文件一页 + `index.md` 索引。
pub fn generate_project(dir: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let manifest = buildzon::load_from_dir(dir).map_err(|e| format!("build.zon 解析失败: {e}"))?;
    let (name, version, kind) = match &manifest {
        Some(m) => (
            if m.name.is_empty() {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("pkg")
                    .to_string()
            } else {
                m.name.clone()
            },
            if m.version.is_empty() {
                "0.1.0".to_string()
            } else {
                m.version.clone()
            },
            match m.kind {
                buildzon::Kind::Exe => "exe",
                buildzon::Kind::Lib => "lib",
                buildzon::Kind::Script => "script",
            }
            .to_string(),
        ),
        None => (
            dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("pkg")
                .to_string(),
            "0.1.0".to_string(),
            "exe".to_string(),
        ),
    };

    let mut files = match &manifest {
        Some(m) if !m.files.is_empty() => m.files.iter().map(|f| dir.join(f)).collect::<Vec<_>>(),
        _ => dir_hc_files(dir),
    };
    files.sort();

    let mut index = String::new();
    index.push_str(&format!(
        "# {name} 文档\n\n版本 {version} · 类型 {kind} · 文件 {}\n\n## 文件\n\n",
        files.len()
    ));
    let mut generated = Vec::new();
    for f in &files {
        let src =
            std::fs::read_to_string(f).map_err(|e| format!("读取 {} 失败: {e}", f.display()))?;
        let rel = f
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled.hc");
        let mut page = render_file_page(rel, &src)?;
        // 索引行摘要：文件文档首行（标题后的正文首行；nav 插入前取，避免混入链接）
        let first_line = page
            .lines()
            .nth(2)
            .map(|l| l.to_string())
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .unwrap_or_default();
        let desc = if first_line.is_empty() {
            String::new()
        } else {
            format!(" — {first_line}")
        };
        // 顶层声明数（### 标题行；#### 嵌套成员不计数）
        let decl_count = page.matches("\n### ").count();
        // H5：项目页导航——标题行后插入「返回索引」链接 + 换行（原空行保留为间隔）
        if let Some(pos) = page.find('\n') {
            page.insert_str(pos + 1, "[← 返回索引](index.md)\n");
        }
        let stem = rel.trim_end_matches(".hc");
        let page_path = write_page(out_dir, &format!("{stem}.md"), &page)?;
        generated.push(page_path);
        index.push_str(&format!("- [{rel}]({stem}.md){desc} · {decl_count} 声明\n"));
    }
    let index_path = write_page(out_dir, "index.md", &index)?;
    generated.push(index_path);
    Ok(generated)
}

// ---------- 标准库页（Rust 内建目录化摘要） ----------

const STDLIB: &[(&str, &[(&str, &str)])] = &[
    (
        "io（I/O）",
        &[
            ("io.print(fmt: String, args...) !void", "格式化输出到 stdout"),
            ("io.env(name: String) ?&[u8]", "环境变量"),
            ("io.stdin / io.stdout / io.stderr", "标准流（可读/写）"),
            ("io.stdout.write_all(data) / io.stderr.write_all(data)", "写真实标准输出/错误流（G2，独立字节流）"),
            ("io.exit(t: ExitType, code: u8) !void", "退出：Exit 静默 / Error 错误退出打印标记"),
        ],
    ),
    (
        "io.fs（文件系统）",
        &[
            ("io.fs.open(path) !File", "打开文件（f.read_all(alloc) / f.write_all(data) / f.close()）"),
            ("io.fs.read_file(path, alloc) !&[u8]", "路径直读"),
            ("io.fs.write_all(&f, data) !void ≡ f.write_all(data)", "句柄写（Q20 双语）"),
            ("io.fs.append(path, data) !void / io.fs.rename(a, b) !void / io.fs.remove(path) !void", "文件增删改"),
            ("io.fs.read_int(path) !i64 / io.fs.write_int(path, v) !void", "整数读写"),
            ("io.fs.open_dir(path) !Dir", "目录句柄（G2：dir.list_dir(alloc) / dir.close()）"),
            ("io.fs.list_dir(path) !Vec(DirEntry)", "目录枚举（G2：{name, is_dir}）"),
            ("f.seek(offset) / f.pos() / f.read_at(buf, offset) / f.write_at(data, offset)", "文件随机访问"),
        ],
    ),
    (
        "io.net（网络）",
        &[
            ("io.net.get(url) !&[u8]", "HTTP GET 客户端（G1；仅 http://，非 200 → error.Http{code}）"),
            ("io.net.connect(host, port, alloc) !TcpConn", "TCP 客户端"),
            ("io.net.listen(host, port, alloc) !TcpListener", "TCP 服务端（accept/read_all/write/shutdown/close/local_port，Q20 双语）"),
            ("io.net.udp.bind(port) !UdpSocket", "UDP（G1；bind(host, port) 亦支持）"),
            ("sock.send_to(addr, data) / sock.recv_from(alloc) ![addr, data] / sock.close()", "UDP 收发（空队列 200ms → error.TimedOut）"),
        ],
    ),
    (
        "io.time（时间）",
        &[
            ("io.time.now() i64", "毫秒时间戳"),
            ("io.time.sleep(ms) void", "休眠"),
            ("io.time.tick() i64 / io.time.elapsed(tick) i64", "单调测量（G5；时区完整留 1.x）"),
        ],
    ),
    (
        "io.text（文本正则，G5）",
        &[
            ("io.text.matches(pattern, text) bool", "是否含匹配（^/$ 锚定控制全串）"),
            ("io.text.find(pattern, text) ?int", "首个匹配起点；无 → null"),
            ("io.text.replace(pattern, text, repl) &[u8]", "替换全部非重叠匹配（每处最长）"),
            ("io.text.split(pattern, text) Vec(&[u8])", "按匹配分割（含空段）"),
            ("子集：字面量 / `.` / `[...]` 范围取反 / `\\d` `\\w` `\\s` / 分组 / `*` `+` `?` `{n,m}` / `|` / `^` `$` / `\\xNN` 转义", "非法模式 → error.InvalidFormat"),
        ],
    ),
    (
        "io.rng（伪随机数，G5）",
        &[
            ("io.rng.seed(v)", "设定种子（0 → 回退默认）"),
            ("io.rng.next() u64", "xorshift64* 原始 64 位"),
            ("io.rng.int(n) int", "[0, n) 均匀（拒绝采样免模偏差）"),
            ("io.rng.float() f64", "[0, 1) 高 53 位"),
        ],
    ),
    (
        "io.ipc（进程内通信，G3）",
        &[
            ("io.ipc.pipe() ![PipeReader, PipeWriter]", "匿名管道（2 元素数组，同 UDP recv_from 约定）"),
            ("reader.read(alloc)/read_all(alloc)/is_closed()/close()；writer.write(data)/close()", "管道读写（空且写端开 → 空切片，不阻塞——协作式）"),
            ("io.ipc.shm(name, size) !Shm", "命名共享内存定长字节区（write/read/close）"),
        ],
    ),
    (
        "io.storage / io.archive（持久化/压缩，G4）",
        &[
            ("io.storage.open(path) !KvStore", "文件持久化键值存储"),
            ("kv.put(key, value) / kv.get(key) !?&[u8]（缺失 → null）/ kv.contains(key) / kv.remove(key) / kv.len() / kv.close()", "键值方法（close 落盘+注销幂等）"),
            ("io.archive.compress(data) !&[u8] / decompress(data) !&[u8]", "RLE 压缩/解压（非法数据 → error.InvalidFormat）"),
        ],
    ),
    (
        "alloc / mem（内存分配）",
        &[
            ("alloc.init(T) T", "类型名/字面量构造实例（带参 `alloc.init(T{...})`）"),
            ("alloc.alloc(size: usize) *u8", "原始分配"),
            ("alloc.free(ptr) !void", "释放"),
            ("mem.Arena.init(alloc) Arena", "Arena 分配器（typed 构造 arena.init(T)）"),
            ("mem.Allocator", "分配器抽象"),
        ],
    ),
    (
        "collections（集合）",
        &[
            ("Vec<T>.init(alloc) Vec<T>", "动态数组"),
            ("Vec<T>.append(v: T)", "追加"),
            ("String.from(bytes: []const u8, alloc) String", "字节 → 字符串"),
            ("String 方法：concat / split / join / find ?usize / substring / replace / to_upper / to_lower / as_slice / to_bytes / == 内容比较", "G2：to_upper/to_lower 为 ASCII 大小写转换（非 ASCII 字节不变）"),
            ("Map<K,V>.init(alloc) Map", "哈希表"),
            ("Deque<T>.init(alloc) Deque", "双端队列"),
        ],
    ),
    (
        "serialize（序列化）",
        &[
            ("serialize.parse_int(s: String) !i64", "十进制 → 整数"),
            ("serialize.parse_float(s: String) !f64", "十进制 → 浮点"),
            ("serialize.json.parse(s: String) !Value", "JSON 解析"),
            ("serialize.csv.parse(s: String) !Vec", "CSV 解析"),
            ("to_bytes(v) []u8 / from_bytes(T, bytes) T", "字节序列化（箱）"),
            ("to_json(v) String / from_json(T, s) T", "JSON 序列化（箱）"),
        ],
    ),
    (
        "scalar 接口族（标量接口）",
        &[
            ("interface ICompare", "比较：lt/le/eq/ne/gt/ge"),
            ("interface INumber: ICompare", "数值运算：add/sub/mul/div/rem/neg/abs"),
            ("interface IInt: INumber", "整数：位运算/移位"),
            ("interface IUint: IInt", "无符号整数"),
            ("interface IFloat: INumber", "浮点：sqrt/pow/floor/ceil/round"),
            ("interface IIterable", "迭代三态（iter/next/done）"),
        ],
    ),
    (
        "@ 内建",
        &[
            ("@sizeOf(T) usize", "类型字节大小"),
            ("@alignOf(T) usize", "类型对齐"),
            ("@offsetOf(T, field) usize", "字段偏移"),
            ("@typeOf(v) type", "值类型"),
            ("@intCast(T, x) T", "整数类型转换"),
            ("@ptrCast(T, p) T", "指针类型转换"),
            ("@ptrFromInt(addr) *mut Unknown", "整数地址 → 虚拟指针"),
            ("@intFromPtr(p) usize", "指针 → 整数地址"),
            ("@volatileLoad(p) T", "防优化掉读穿（MMIO）"),
            ("@volatileStore(p, v)", "防优化掉写穿（MMIO）"),
            ("@compileError(msg)", "编译期错误"),
            ("@addWithOverflow(a, b) (T, bool)", "溢出检测加法"),
            ("@panic(msg)", "运行时中止"),
            ("box(v) / copy(v)", "装箱/复制"),
        ],
    ),
    (
        "线程（组 G 生命周期）",
        &[
            ("spawn(f, args...) o Thread(T)", "协作式延迟执行：立即返回句柄，join 时运行"),
            ("thread.join() !T", "运行到完成并取结果"),
            ("thread.cancel() !void", "协作取消（未运行 → join 返回 error.Cancelled）"),
            ("thread.is_done() bool", "完成查询"),
            ("thread.detach()", "立即运行到完成并丢弃结果"),
        ],
    ),
];

/// 生成标准库页 → `out_dir/std.md`（内置目录化摘要）。
pub fn generate_stdlib(out_dir: &Path) -> Result<PathBuf, String> {
    let mut out = String::new();
    out.push_str("# H 标准库文档\n\n");
    out.push_str(
        "H.std 标准库为编译器内建（Rust 实现，无 .hc 源）；本页为目录化摘要（覆盖 tag1 已落地子集，非完整 API）。`import H.std.{io}` 显式引入对应模块。\n\n",
    );
    for (module, entries) in STDLIB {
        out.push_str(&format!("## {module}\n\n"));
        for (sig, doc) in *entries {
            out.push_str(&format!("- `{sig}` — {doc}\n"));
        }
        out.push('\n');
    }
    write_page(out_dir, "std.md", &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_run_extraction() {
        let src = "/// 文件头\n/// 第二行\n\n/// fn 文档\nfn f() i32 { return 1; }\n";
        let runs = collect_doc_runs(src);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "文件头\n第二行");
        assert_eq!(runs[1].text, "fn 文档");
    }

    #[test]
    fn page_contains_sig_and_doc() {
        let src = "import H.std.{io};\n\n/// 入口函数\nfn main(args: o Vec<String>) !void {\n    io.print(\"hi\\n\");\n}\n";
        let page = render_file_page("main.hc", src).unwrap();
        assert!(page.contains("# `main`"), "page: {page}");
        assert!(page.contains("fn main(args: o Vec<String>) !void"));
        assert!(page.contains("入口函数"));
        assert!(page.contains("import H.std.{io};"));
    }

    #[test]
    fn stdlib_page_has_modules() {
        let mut out = String::new();
        for (module, _) in STDLIB {
            out.push_str(module);
        }
        assert!(out.contains("io（I/O）"));
        assert!(out.contains("线程（组 G 生命周期）"));
    }

    #[test]
    fn render_type_covers_forms() {
        assert_eq!(render_type(&Type::Named("i32".into(), vec![])), "i32");
        assert_eq!(
            render_type(&Type::Named(
                "Vec".into(),
                vec![Type::Named("i32".into(), vec![])]
            )),
            "Vec<i32>"
        );
        assert_eq!(
            render_type(&Type::Owned(Box::new(Type::Named("String".into(), vec![])))),
            "o String"
        );
        assert_eq!(
            render_type(&Type::ErrorUnion(
                None,
                Box::new(Type::Named("void".into(), vec![]))
            )),
            "!void"
        );
        assert_eq!(
            render_type(&Type::Array(2, Box::new(Type::Named("i32".into(), vec![])))),
            "[2]i32"
        );
    }
}
