//! `hc build`：原生编译（LLVM IR + zig cc）/ 库构建（静态归档 / dll）/ 字节码回退。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hc::ast::{Decl, Expr};

use crate::buildzon;
use crate::cli::{err_color, paint};
use crate::fsio::{link_exe, source_to_bytecode, write_bytecode_artifact, zig_cc_available};
use crate::package::{
    check_and_merge, check_and_merge_deps, dir_hc_files, package_entry, package_programs,
    strip_test_funcs_in_place,
};
use crate::project::resolve_registry_dep;

/// C3/C4：`Kind::lib` 构建——codegen_lib（包前缀，剔除 test 函数；静态归档转 runtime
/// helper 为 declare，dll 保持自包含）→ `zig cc -c` + `zig ar rcs lib{name}.a`（静态）
/// 或 `zig cc -shared` → `{name}.dll`（dll，exe 运行时加载）。另写 `{name}.sym`
/// （限定名 → 导出符号，exe 链接引用）。**库无 main 校验**（C4：Kind::lib 含 main → 诊断）。
/// 返回（库文件路径，符号表）。
fn build_lib(
    dir: &Path,
    name: &str,
    dll: bool,
) -> Result<(PathBuf, Vec<(String, String)>), ExitCode> {
    let entry_path = match package_entry(dir) {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("error: {msg}");
            return Err(ExitCode::FAILURE);
        }
    };
    let (source, entry, siblings) = match package_programs(&entry_path) {
        Ok(t) => t,
        Err(c) => return Err(c),
    };
    let sib_refs: Vec<&hc::Program> = siblings.iter().collect();
    let (merged, table) = match check_and_merge(&entry, &source, &sib_refs, false) {
        Ok(t) => t,
        Err(msg) => {
            eprint!("{msg}");
            return Err(ExitCode::FAILURE);
        }
    };
    // 库不运行测试：剔除 [test] 函数（其断言 helper 在库形态不发射，保留会链接失败）
    let mut merged = merged;
    strip_test_funcs_in_place(&mut merged);
    // C4：库无 main 校验——`Kind::lib` = 不含 main 的包（06-08 定案）
    if merged.func_index.contains_key("main") {
        eprintln!(
            "error: 库包 `{}` 不应含 `main` 入口（Kind::lib = 不含 main 的包；应用请用 Kind.exe）",
            name
        );
        return Err(ExitCode::FAILURE);
    }
    // 符号表：func_index 限定名 → `{pkg}.hc_fn{i}`（pub 边界由语义层 import 检查保证——
    // 非 pub 符号 import 即报错，.sym 全量导出不绕过）
    let mut syms: Vec<(String, String)> = merged
        .func_index
        .iter()
        .map(|(fn_, idxs)| (format!("{name}.{fn_}"), format!("{name}.hc_fn{}", idxs[0])))
        .collect();
    syms.sort();
    let ll = hc::llvm::codegen_lib(&merged, &table, name, dll);
    let ll_path = dir.join(format!("{name}.ll"));
    if let Err(e) = std::fs::write(&ll_path, &ll) {
        eprintln!("error: 写入 {} 失败: {e}", ll_path.display());
        return Err(ExitCode::FAILURE);
    }
    let artifact = if dll {
        // C4：dll 动态库——`zig cc -shared`；自包含 helper（codegen_lib dll_mode）
        let dll_path = dir.join(format!("{name}.dll"));
        let shared = std::process::Command::new("zig")
            .arg("cc")
            .arg("-shared")
            .arg(&ll_path)
            .arg("-o")
            .arg(&dll_path)
            .output();
        match shared {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                eprintln!(
                    "{} zig cc -shared 失败：\n{}",
                    paint(err_color(), "31", "[FAIL]"),
                    String::from_utf8_lossy(&o.stderr)
                );
                return Err(ExitCode::FAILURE);
            }
            Err(e) => {
                eprintln!("调用 zig cc 失败: {e}");
                return Err(ExitCode::FAILURE);
            }
        }
        dll_path
    } else {
        // C3：静态归档——`zig cc -c` → `.o` → `zig ar rcs lib{name}.a`
        let o_path = dir.join(format!("{name}.o"));
        let cc = std::process::Command::new("zig")
            .arg("cc")
            .arg("-c")
            .arg(&ll_path)
            .arg("-o")
            .arg(&o_path)
            .output();
        match cc {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                eprintln!(
                    "{} zig cc -c 失败：\n{}",
                    paint(err_color(), "31", "[FAIL]"),
                    String::from_utf8_lossy(&o.stderr)
                );
                return Err(ExitCode::FAILURE);
            }
            Err(e) => {
                eprintln!("调用 zig cc 失败: {e}");
                return Err(ExitCode::FAILURE);
            }
        }
        let a_path = dir.join(format!("lib{name}.a"));
        let ar = std::process::Command::new("zig")
            .arg("ar")
            .arg("rcs")
            .arg(&a_path)
            .arg(&o_path)
            .output();
        if let Err(e) = ar.and_then(|o| {
            if o.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("zig ar: {}", String::from_utf8_lossy(&o.stderr)),
                ))
            }
        }) {
            eprintln!("error: 归档 {} 失败: {e}", a_path.display());
            return Err(ExitCode::FAILURE);
        }
        let _ = std::fs::remove_file(&o_path);
        a_path
    };
    // 符号表文件（构建产物；exe 链接侧已由返回值直接使用）
    let sym_text = syms
        .iter()
        .map(|(qn, sym)| format!("{qn} {sym}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(dir.join(format!("{name}.sym")), format!("{sym_text}\n"));
    // 清理中间 .ll，保留产物 + .sym
    let _ = std::fs::remove_file(&ll_path);
    Ok((artifact, syms))
}

/// `hc build [--dll] <path>`：M3.3 原生编译（emit-.ll + `zig cc` → 可执行文件）。
/// `zig cc` 缺失时回退字节码镜像 .hbc + 平台启动器（M3.2 前过渡形态）。
/// Q13：`hc build <dir>` 验证——目录必须含 build.zon + main.hc。
/// C3：`Kind::lib` → 静态归档（build_lib）；`Kind::exe` 带本地依赖 → 链接库形态。
/// C4：`--dll` → `Kind::lib` 产 dll（`zig cc -shared`，自包含 helper）；`Kind::exe`
/// 依赖库按 dll 构建并**链接 dll**（OS 运行时加载；dll 复制到 exe 目录供加载器定位）。
/// Q13：`hc build <dir>` 验证——目录必须含 build.zon + main.hc。
/// 对 `hc build <file>` 单文件路径，`entry_path` 直接为该文件。
fn resolve_build_entry(path: &Path) -> Result<PathBuf, ExitCode> {
    if path.is_dir() {
        // Q13：目录参数必须含 build.zon + main.hc
        if !path.join("build.zon").exists() {
            eprintln!(
                "error: 目录 {} 缺少 build.zon（项目清单；`hc build <dir>` 需项目目录）",
                path.display()
            );
            return Err(ExitCode::FAILURE);
        }
        match package_entry(path) {
            Ok(entry) => Ok(entry),
            Err(msg) => {
                eprintln!("error: {msg}");
                Err(ExitCode::FAILURE)
            }
        }
    } else {
        Ok(path.to_path_buf())
    }
}

/// Auto-increment build number in version.hc after successful build.
/// Returns the new build number, or 0 if no version.hc exists.
fn update_version_hc(dir: &Path) -> Result<u64, String> {
    let vpath = dir.join("version.hc");
    if !vpath.exists() {
        return Ok(0);
    }
    let content =
        std::fs::read_to_string(&vpath).map_err(|e| format!("读取 version.hc 失败: {e}"))?;

    // 用 H parser 解析以提取当前 build 号
    let program = hc::parse_source(&content).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    })?;

    let mut old_build = 0u64;
    let mut old_time = 0u64;
    let mut has_time = false;
    let mut found = false;
    for decl in &program.decls {
        if let Decl::Const { name, init, .. } = decl {
            if name == "version" {
                if let Expr::NamedLit { ty, fields, .. } = init {
                    if ty == "Version" {
                        for (key, val) in fields {
                            if key == "build" {
                                if let Expr::IntLit { text, .. } = val {
                                    old_build = text.parse::<u64>().unwrap_or(0);
                                    found = true;
                                }
                            }
                            if key == "time" {
                                has_time = true;
                                if let Expr::IntLit { text, .. } = val {
                                    old_time = text.parse::<u64>().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if !found {
        return Ok(0);
    }

    let new_build = old_build + 1;
    let new_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 逐行处理以安全替换
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut build_replaced = false;
    let mut _time_replaced = false;

    for i in 0..lines.len() {
        let trimmed = lines[i].trim().to_string();
        if trimmed.starts_with("build = ") && trimmed.ends_with(',') {
            let num_part = trimmed
                .strip_prefix("build = ")
                .unwrap_or("")
                .strip_suffix(',')
                .unwrap_or("")
                .trim()
                .to_string();
            if let Ok(n) = num_part.parse::<u64>() {
                if n == old_build {
                    let indent =
                        lines[i][..lines[i].len() - lines[i].trim_start().len()].to_string();
                    lines[i] = format!("{}build = {},", indent, new_build);
                    build_replaced = true;
                }
            }
        }
        if has_time && trimmed.starts_with("time = ") && trimmed.ends_with(',') {
            let num_part = trimmed
                .strip_prefix("time = ")
                .unwrap_or("")
                .strip_suffix(',')
                .unwrap_or("")
                .trim()
                .to_string();
            if let Ok(n) = num_part.parse::<u64>() {
                if n == old_time {
                    let indent =
                        lines[i][..lines[i].len() - lines[i].trim_start().len()].to_string();
                    lines[i] = format!("{}time = {},", indent, new_time);
                    _time_replaced = true;
                }
            }
        }
    }

    // 无 time 字段时在 build 行后插入
    if !has_time && build_replaced {
        for i in 0..lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("build = ") && trimmed.ends_with(',') {
                let indent = &lines[i][..lines[i].len() - lines[i].trim_start().len()];
                lines.insert(i + 1, format!("{}time = {},", indent, new_time));
                break;
            }
        }
    }

    let new_content = lines.join("\n");
    std::fs::write(&vpath, &new_content).map_err(|e| format!("写入 version.hc 失败: {e}"))?;

    println!(
        "  version.hc: build {} → {}  (time updated)",
        old_build, new_build
    );
    Ok(new_build)
}

/// 更新 version.hc 的包装函数：build 成功时调用，失败仅打印警告不阻断构建。
fn try_update_version_hc(dir: &Path) {
    match update_version_hc(dir) {
        Ok(n) if n > 0 => {} // 已打印信息
        Ok(_) => {}          // 无 version.hc
        Err(msg) => eprintln!("[warn] version.hc 更新失败: {msg}"),
    }
}

pub(crate) fn build_file(path: &Path, dll: bool) -> ExitCode {
    // Q13：目录参数必须含 build.zon + main.hc
    let entry_path = match resolve_build_entry(path) {
        Ok(p) => p,
        Err(c) => return c,
    };

    let (source, entry, siblings) = match package_programs(&entry_path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let stem = entry_path.file_stem().unwrap_or_default().to_string_lossy();
    let dir = entry_path.parent().unwrap_or_else(|| Path::new("."));
    // M4-1：项目根目录（version.hc 所在位置）——传入的 path 为目录时就是项目根
    let project_root = if path.is_dir() { path } else { dir };

    // M7.2：build.zon 包清单（C3：kind 分流——lib → 静态归档；exe → 链接依赖库）
    let manifest = match buildzon::load_from_dir(dir) {
        Ok(Some(m)) => {
            println!("包：{} {}（{:?}）", m.name, m.version, m.kind);
            if !m.files.is_empty() {
                println!("  文件：{}", m.files.join(", "));
            }
            if !m.deps.is_empty() {
                let names: Vec<&str> = m.deps.iter().map(|d| d.name.as_str()).collect();
                println!("  依赖：{}", names.join(", "));
            }
            Some(m)
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("[warn] build.zon 解析失败: {e}");
            None
        }
    };

    // C3/C4：库形态——不产出 exe，编译为静态归档（zig cc -c + zig ar）或 dll（--dll，
    // zig cc -shared）
    if manifest
        .as_ref()
        .is_some_and(|m| m.kind == buildzon::Kind::Lib)
    {
        if !zig_cc_available() {
            eprintln!("error: 库构建需要 zig cc（当前未检测到）");
            return ExitCode::FAILURE;
        }
        return match build_lib(dir, &manifest.as_ref().unwrap().name, dll) {
            Ok((a_path, _)) => {
                println!("库产物: {}", a_path.display());
                try_update_version_hc(project_root);
                ExitCode::SUCCESS
            }
            Err(c) => c,
        };
    }

    // M3.3 原生路径：zig cc 可用 → 生成 .ll → 编译链接为可执行文件
    if zig_cc_available() {
        let sib_refs: Vec<&hc::Program> = siblings.iter().collect();
        // C3：本地依赖先构建为静态库，收集 .a 与外部符号表（限定名 → `{pkg}.hc_fn{i}`）
        let mut libs: Vec<PathBuf> = Vec::new();
        let mut links: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut dep_progs: Vec<(String, hc::Program)> = Vec::new();
        if let Some(m) = &manifest {
            for dep in &m.deps {
                let dep_dir = if let Some(rel) = &dep.path {
                    dir.join(rel)
                } else {
                    // B3：注册中心依赖——从 ~/.hc/registry/<name>/<version>/ 解析
                    match resolve_registry_dep(&dep.name, &dep.version, dep.fingerprint.as_deref())
                    {
                        Ok((reg_dir, _)) => reg_dir,
                        Err(msg) => {
                            eprintln!("error: {msg}");
                            return ExitCode::FAILURE;
                        }
                    }
                };
                // H2：缺失依赖诊断——本地依赖 path 须指向存在的完整包
                if !dep_dir.is_dir() {
                    eprintln!(
                        "error: 依赖 {} 路径不存在: {}（本地依赖 path 须指向包目录）",
                        dep.name,
                        dep_dir.display()
                    );
                    return ExitCode::FAILURE;
                }
                if let Ok(Some(dm)) = buildzon::load_from_dir(&dep_dir) {
                    // H2：版本声明检查（本地 path 权威，不符告警）
                    if !dep.version.is_empty()
                        && !dm.version.is_empty()
                        && dep.version != dm.version
                    {
                        eprintln!(
                            "[warn] 依赖 {} 声明版本 {} 与本地 {} 不符",
                            dep.name, dep.version, dm.version
                        );
                    }
                }
                match build_lib(&dep_dir, &dep.name, dll) {
                    Ok((art, syms)) => {
                        // C4：dll 模式把依赖 dll 复制到 exe 目录——Windows 加载器仅在
                        // exe 目录/系统路径/PATH 搜索，子目录不找；静态归档直接链接无需复制
                        let link_path = if dll {
                            let copy = dir.join(art.file_name().unwrap_or_default());
                            let _ = std::fs::copy(&art, &copy);
                            copy
                        } else {
                            art
                        };
                        libs.push(link_path);
                        for (qn, sym) in syms {
                            links.insert(qn, sym);
                        }
                    }
                    Err(c) => return c,
                }
                // 依赖包源码（pub 符号登记用，不合并函数体）
                for f in dir_hc_files(&dep_dir) {
                    if let Ok(src) = std::fs::read_to_string(&f) {
                        if let Ok(p) = hc::parse_source(&src) {
                            dep_progs.push((dep.name.clone(), p));
                        }
                    }
                }
            }
        }
        let dep_refs: Vec<(&str, &hc::Program)> =
            dep_progs.iter().map(|(n, p)| (n.as_str(), p)).collect();
        let ll = match check_and_merge_deps(&entry, &source, &sib_refs, false, &dep_refs)
            .and_then(|(merged, table)| Ok(hc::llvm::codegen_with_links(&merged, &table, &links)))
        {
            Ok(ll) => ll,
            Err(msg) => {
                eprint!("{msg}");
                return ExitCode::FAILURE;
            }
        };
        let ll_path = dir.join(format!("{stem}.ll"));
        if let Err(e) = std::fs::write(&ll_path, &ll) {
            eprintln!("error: 写入 {} 失败: {e}", ll_path.display());
            return ExitCode::FAILURE;
        }
        let exe_name = if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.to_string()
        };
        let exe_path = dir.join(&exe_name);
        if let Err(msg) = link_exe(&ll_path, &exe_path, &libs) {
            eprintln!("{} {msg}", paint(err_color(), "31", "[FAIL]"));
            eprintln!("（LLVM IR 已保留：{}）", ll_path.display());
            return ExitCode::FAILURE;
        }
        // 编译成功：清理中间 .ll，保留可执行文件
        let _ = std::fs::remove_file(&ll_path);
        println!("原生产物: {}", exe_path.display());
        try_update_version_hc(project_root);
        return ExitCode::SUCCESS;
    }

    // 回退：真实 HBC2 字节码 + 平台启动器（zig cc 缺失；M3.2 字节码 VM）
    eprintln!("[warn] 未检测到 zig cc——回退字节码 VM（M3.2 全语言；原生后端需要 zig）");
    if !siblings.is_empty() {
        eprintln!(
            "[warn] 检测到 {} 个同包兄弟文件——字节码回退仅编译入口文件，多文件需 zig 原生后端",
            siblings.len()
        );
    }
    let bytecode = match source_to_bytecode(&source) {
        Ok(b) => b,
        Err(msg) => {
            eprint!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let hbc_path = match write_bytecode_artifact(dir, &stem, &bytecode) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let runner = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hc-tools"));
    let launcher = if cfg!(windows) {
        let l = dir.join(format!("{stem}.cmd"));
        let content = format!(
            "@echo off\r\nrem H 语言字节码启动器（tag1：由字节码 VM 加载 .hbc）\r\n\"{}\" run \"{}\"\r\n",
            runner.display(),
            hbc_path.display()
        );
        let _ = std::fs::write(&l, content);
        l
    } else {
        let l = dir.join(format!("{stem}.sh"));
        let content = format!(
            "#!/bin/sh\nexec \"{}\" run \"{}\"\n",
            runner.display(),
            hbc_path.display()
        );
        let _ = std::fs::write(&l, content);
        l
    };

    println!("编译产物：");
    println!("  字节码    : {}", hbc_path.display());
    println!("  启动器    : {}", launcher.display());
    println!("运行方式：{}", launcher.display());
    try_update_version_hc(project_root);
    ExitCode::SUCCESS
}
