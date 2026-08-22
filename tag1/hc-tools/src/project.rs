//! `hc init` / `hc pkg add` / `hc pkg publish`：项目骨架生成、build.zon 依赖写入、注册中心发布。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::ast::{Decl, Expr};
use sha2::{Digest, Sha256};

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
         fn main() !void {{\n\
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

/// `~/.hc` 注册中心根目录（跨平台：Unix `$HOME` / Windows `%USERPROFILE%`）。
pub(crate) fn registry_root() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".hc").join("registry")
}

/// 注册中心依赖路径：`~/.hc/registry/<name>/<version>/`
pub(crate) fn registry_dep_path(name: &str, version: &str) -> PathBuf {
    registry_root().join(name).join(version)
}

/// 计算一组文件的 SHA-256 指纹（按相对路径排序后依次哈希 `{path}\n{content}`，
/// 替换换行符确保跨平台一致）。
fn compute_fingerprint(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = Sha256::new();
    for (path, content) in files {
        hasher.update(path.as_bytes());
        hasher.update(b"\n");
        hasher.update(content);
    }
    hex::encode(hasher.finalize())
}

/// B3：`hc pkg publish`——从当前目录发布包到本地注册中心。
///
/// 1. 读取并解析 build.zon（缺失/无效报错）
/// 2. 收集包文件（`files` 字段或全部 `.hc` 文件）
/// 3. 计算 SHA-256 指纹
/// 4. 创建 `~/.hc/registry/<name>/<version>/` 目录
/// 5. 写入 `manifest.zon`（含指纹）+ 复制包文件
/// 6. 输出确认信息
pub(crate) fn pkg_publish() -> ExitCode {
    let zon_path = Path::new("build.zon");
    let src = match std::fs::read_to_string(zon_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: 当前目录无 build.zon（先 `hc init <name>` 创建项目）");
            return ExitCode::FAILURE;
        }
    };
    let manifest = match crate::buildzon::parse(&src) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: build.zon 解析失败: {e}");
            return ExitCode::FAILURE;
        }
    };
    if manifest.name.is_empty() {
        eprintln!("error: build.zon 缺少 `name` 字段");
        return ExitCode::FAILURE;
    }
    if manifest.version.is_empty() {
        eprintln!("error: build.zon 缺少 `version` 字段");
        return ExitCode::FAILURE;
    }

    // 收集文件：`files` 字段优先，否则全部 `.hc` 文件
    let files: Vec<PathBuf> = if manifest.files.is_empty() {
        let mut v = Vec::new();
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "hc") {
                    v.push(p);
                }
            }
        }
        v.sort();
        v
    } else {
        manifest
            .files
            .iter()
            .map(|f| Path::new(f).to_path_buf())
            .collect()
    };

    if files.is_empty() {
        eprintln!("error: 无包文件可发布（build.zon `files` 为空且目录无 `.hc` 文件）");
        return ExitCode::FAILURE;
    }

    // 读取文件内容并计算指纹
    let mut file_map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for f in &files {
        let content = match std::fs::read(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: 读取 {} 失败: {e}", f.display());
                return ExitCode::FAILURE;
            }
        };
        let rel = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        file_map.insert(rel, content);
    }
    let fingerprint = compute_fingerprint(&file_map);

    // 创建注册中心目录
    let reg_dir = registry_dep_path(&manifest.name, &manifest.version);
    if let Err(e) = std::fs::create_dir_all(&reg_dir) {
        eprintln!("error: 创建注册中心目录 {} 失败: {e}", reg_dir.display());
        return ExitCode::FAILURE;
    }

    // 写入 manifest.zon（含指纹）
    let manifest_zon = format!(
        "const build = Build{{\n    name = \"{}\",\n    version = \"{}\",\n    kind = {},\n    files = [\n        {},\n    ],\n    deps = [],\n}};\n// 注册中心指纹——SHA-256\nconst fingerprint = \"{}\";\n",
        manifest.name,
        manifest.version,
        match manifest.kind {
            crate::buildzon::Kind::Exe => "Kind.exe",
            crate::buildzon::Kind::Lib => "Kind.lib",
            crate::buildzon::Kind::Script => "Kind.script",
        },
        files
            .iter()
            .map(|f| {
                f.file_name()
                    .map(|n| format!("\"{}\"", n.to_string_lossy()))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",\n        "),
        fingerprint
    );
    if let Err(e) = std::fs::write(reg_dir.join("manifest.zon"), &manifest_zon) {
        eprintln!("error: 写入 manifest.zon 失败: {e}");
        return ExitCode::FAILURE;
    }

    // 复制包文件
    for (rel, content) in &file_map {
        let dest = reg_dir.join(rel);
        if let Err(e) = std::fs::write(&dest, content) {
            eprintln!("error: 写入 {} 失败: {e}", dest.display());
            return ExitCode::FAILURE;
        }
    }

    println!(
        "已发布 `{}@{}` 到本地注册中心：",
        manifest.name, manifest.version
    );
    println!("  目录: {}", reg_dir.display());
    println!("  文件: {} 个", file_map.len());
    println!("  指纹: {}", fingerprint);
    ExitCode::SUCCESS
}

/// 解析注册中心 `manifest.zon` 中的指纹。
fn parse_registry_fingerprint(src: &str) -> Result<String, String> {
    let program = hc::parse_source(src).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    for decl in &program.decls {
        if let Decl::Const { name, init, .. } = decl {
            if name == "fingerprint" {
                if let Expr::StrLit { value, .. } = init {
                    return Ok(value.clone());
                }
            }
        }
    }
    Err("manifest.zon: 缺少 `const fingerprint = \"...\"`".into())
}

/// B3：从注册中心解析依赖——返回（依赖目录，指纹）。
/// 目录不存在/指纹不匹配返回 Err 文本。
pub(crate) fn resolve_registry_dep(
    name: &str,
    version: &str,
    expected_fingerprint: Option<&str>,
) -> Result<(PathBuf, String), String> {
    let reg_dir = registry_dep_path(name, version);
    if !reg_dir.is_dir() {
        return Err(format!(
            "依赖 `{name}@{version}` 未在注册中心找到（{}）——先 `hc pkg publish`",
            reg_dir.display()
        ));
    }

    // 读取并解析 manifest.zon
    let manifest_src = std::fs::read_to_string(reg_dir.join("manifest.zon"))
        .map_err(|_| format!("注册中心 {} 缺少 manifest.zon", reg_dir.display()))?;
    let fingerprint = parse_registry_fingerprint(&manifest_src)?;

    // 校验指纹
    let mut file_map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(&reg_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "hc") {
                if let Ok(content) = std::fs::read(&p) {
                    if let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) {
                        file_map.insert(name, content);
                    }
                }
            }
        }
    }
    let actual = compute_fingerprint(&file_map);
    if actual != fingerprint {
        return Err(format!(
            "依赖 `{name}@{version}` 指纹不匹配！\n  期望: {}\n  实际: {}\n  文件可能已被篡改或损坏",
            fingerprint, actual
        ));
    }

    // 如有期望指纹，额外校验
    if let Some(expected) = expected_fingerprint {
        if actual != expected {
            return Err(format!(
                "依赖 `{name}@{version}` 指纹校验失败！\n  build.zon 声明: {}\n  注册中心实际: {}\n  供应链校验不通过",
                expected, actual
            ));
        }
    }

    Ok((reg_dir, actual))
}
