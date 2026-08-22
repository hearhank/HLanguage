//! `hc init` / `hc pkg add`：项目骨架生成与 build.zon 依赖写入。

use std::path::Path;
use std::process::ExitCode;

use hc::ast::{Decl, Expr};

/// H1：`hc init <name>`——在当前目录生成最小项目骨架（`build.zon` + `main.hc`）。
///
/// 约定见 `docs/SPEC/06-13-project-structure.md`：目录 = 包、源码位于包根、
/// 测试 = `[test]` 标注函数与源码同文件、依赖经 build.zon deps。脚手架即
/// 最小可运行示例——`hc run <name>` / `hc test <name>` 全绿（CLI 测试保证）。
/// 安全：目录已存在且非空 → 拒绝覆盖（不触碰现有文件）。
pub(crate) fn init_project(name: &str) -> ExitCode {
    // 名称校验：合法目录名（字母/数字/`-`/`_`；非空、非 `.`/`..`）
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eprintln!("error: `hc init` 名称 `{name}` 非法（允许字母/数字/`-`/`_`）");
        return ExitCode::from(2);
    }
    let dir = Path::new(name);
    if dir.exists() {
        let non_empty = std::fs::read_dir(dir)
            .map(|mut it| it.next().is_some())
            .unwrap_or(true);
        if non_empty {
            eprintln!("error: 目录 `{name}` 已存在且非空——拒绝覆盖（请换用空目录或新名）");
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: 创建目录 `{name}` 失败: {e}");
        return ExitCode::FAILURE;
    }
    let zon = format!(
        "// build.zon — {name} 包清单（hc init 脚手架）\n\
         //\n\
         // 清单即数据：const build = Build{{ ... }}（H 数据字面量，Q26）\n\
         //   - name/version/kind：包标识与形态（exe = 应用，含 main 入口）\n\
         //   - files：包内文件清单（源码位于包根，见 06-13-project-structure.md）\n\
         //   - deps：依赖声明（本地依赖带 path；`hc pkg add <name> --path <dir>` 写入）\n\
         \n\
         const build = Build{{\n\
         \x20   name = \"{name}\",\n\
         \x20   version = \"0.1.0\",\n\
         \x20   kind = Kind.exe,\n\
         \x20   files = [ \"main.hc\", ],\n\
         \x20   deps = [],\n\
         }};\n"
    );
    let main_hc = format!(
        "import H.std.{{io}};\n\
         \n\
         // {name}/main.hc — 项目入口（hc init 脚手架）\n\
         //\n\
         //   - 源码约定：`.hc` 文件位于包根（目录 = 包，M1.4）\n\
         //   - 测试约定：`[test]` 标注函数与源码同文件（Q-T1）\n\
         //   - 运行：`hc run {name}`   测试：`hc test {name}`\n\
         \n\
         fn main(args: o Vec<String>) !void {{\n\
         \x20   io.print(\"hello, {name}!\\n\");\n\
         }}\n\
         \n\
         [test] fn scaffold_smoke() !void {{\n\
         \x20   try expect_eq(1 + 1, 2);\n\
         }}\n"
    );
    if let Err(e) = std::fs::write(dir.join("build.zon"), &zon) {
        eprintln!("error: 写入 {} 失败: {e}", dir.join("build.zon").display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(dir.join("main.hc"), &main_hc) {
        eprintln!("error: 写入 {} 失败: {e}", dir.join("main.hc").display());
        return ExitCode::FAILURE;
    }
    println!("创建项目 `{name}`：");
    println!("  {name}/build.zon");
    println!("  {name}/main.hc");
    println!("运行：hc run {name}   测试：hc test {name}");
    ExitCode::SUCCESS
}

/// 从 `start` 起的源文本中查找字符 `ch`（build.zon 数据字面量字段为简单标量，
/// 字符串值不含 `{`/`}`/`[`/`]`，粗扫描足够）。
fn find_char(src: &str, start: usize, ch: u8) -> Option<usize> {
    src.as_bytes()
        .get(start..)
        .and_then(|b| b.iter().position(|&c| c == ch))
        .map(|i| start + i)
}

/// 从 `open` 指向的 `{` 起找匹配的 `}`（深度计数，返回 `}` 字节偏移）。
fn find_block_close(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// H2：定位 build.zon 的 `const build = Build{...}` 中 deps 数组。
/// 返回（deps 数组 `[` 偏移、现有 Pkg 项（name, 干净 `Pkg{...}` span）、Build 字面量 span）。
/// 解析失败返回可读错误文本；无 deps 字段时数组偏移 = None。
///
/// 注：parser 的 ArrayLit/NamedLit span.end 取 `]`/`}` 之后下一个 token（常含尾随
/// 逗号），故本函数按字符扫描重算干净块边界（`find_block_close`）。
fn locate_build_deps(
    src: &str,
) -> Result<(Option<usize>, Vec<(String, (usize, usize))>, (usize, usize)), String> {
    let program = hc::parse_source(src).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let init = program
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::Const { name, init, .. } if name == "build" => Some(init),
            _ => None,
        })
        .ok_or_else(|| "build.zon: 缺少 `const build = Build{ ... }`".to_string())?;
    let Expr::NamedLit {
        ty,
        fields,
        span: build_span,
        ..
    } = init
    else {
        return Err("build.zon: `build` 必须是 `Build{ ... }` 数据字面量".into());
    };
    if ty != "Build" {
        return Err("build.zon: `build` 必须是 `Build{ ... }` 数据字面量".into());
    }
    let build_span = (build_span.start, build_span.end);
    for (key, val) in fields {
        if key == "deps" {
            if let Expr::ArrayLit(items, span) = val {
                let array_open = span.start; // `[` 偏移
                let mut pkgs = Vec::new();
                for item in items {
                    if let Expr::NamedLit {
                        ty, fields, span, ..
                    } = item
                    {
                        if ty == "Pkg" {
                            let mut pkg_name = String::new();
                            for (k, v) in fields {
                                if k == "name" {
                                    if let Expr::StrLit { value, .. } = v {
                                        pkg_name = value.clone();
                                    }
                                }
                            }
                            // 干净 Pkg 块：`Pkg` 起始 .. 匹配 `}` 之后
                            let brace = find_char(src, span.start, b'{').unwrap_or(span.start);
                            let close = find_block_close(src, brace).unwrap_or(span.end);
                            pkgs.push((pkg_name, (span.start, close + 1)));
                        }
                    }
                }
                return Ok((Some(array_open), pkgs, build_span));
            }
        }
    }
    Ok((None, Vec::new(), build_span))
}

/// H2：`hc pkg add <name> [--path <dir>] [--version <ver>]`——在当前目录 build.zon
/// 的 deps 数组中写入/更新本地依赖声明（`Pkg{ name, version, path }`）。
///
/// 缺失 build.zon 报错（提示先 `hc init`）；保留数组外注释与格式；已存在同名依赖
/// → 替换其 Pkg 项（更新 path/version）。deps 数组按「既有 Pkg 原文 + 新项」重建。
pub(crate) fn pkg_add(name: &str, path: &Option<String>, version: &Option<String>) -> ExitCode {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        eprintln!("error: `hc pkg add` 包名 `{name}` 非法（允许字母/数字/`-`/`_`）");
        return ExitCode::from(2);
    }
    let zon_path = Path::new("build.zon");
    let src = match std::fs::read_to_string(zon_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: 当前目录无 build.zon（先 `hc init <name>` 创建项目）");
            return ExitCode::FAILURE;
        }
    };
    let (array_open, pkgs, build_span) = match locate_build_deps(&src) {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let ver = version
        .clone()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.1.0".to_string());
    let path_s = path.clone().unwrap_or_default();
    let entry = format!("Pkg{{ name = \"{name}\", version = \"{ver}\", path = \"{path_s}\", }}");
    let mut out = src;
    match array_open {
        Some(open) => {
            // 重建 deps 数组：既有 Pkg 原文（替换同名项）+ 新项
            let close = find_char(&out, open, b']').unwrap_or(out.len().saturating_sub(1));
            let mut inner = String::new();
            let mut replaced = false;
            for (n, (ps, pe)) in &pkgs {
                if n == name {
                    inner.push_str(&format!("    {entry},\n"));
                    replaced = true;
                } else {
                    inner.push_str(&format!("    {},\n", &out[*ps..*pe]));
                }
            }
            if !replaced {
                inner.push_str(&format!("    {entry},\n"));
            }
            out.replace_range(open..=close, &format!("[\n{inner}]"));
        }
        None => {
            // 无 deps 字段：在 Build 字面量 `}` 前插入 deps 字段
            let brace = find_char(&out, build_span.0, b'{').unwrap_or(build_span.0);
            let close = find_block_close(&out, brace).unwrap_or(build_span.1);
            out.insert_str(close, &format!("\n    deps = [ {entry}, ],"));
        }
    }
    match std::fs::write(zon_path, &out) {
        Ok(()) => {
            println!("依赖 `{name}` 已写入 build.zon deps（version {ver}，path `{path_s}`）");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: 写入 build.zon 失败: {e}");
            ExitCode::FAILURE
        }
    }
}
