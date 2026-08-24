//! 脚本解析器：.hs 脚本文件解析、缓存与 SDK 路径管理

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use hc::ast::{Decl, Program};
use hc::diag;
use hc::token::Span;

/// 装载路径统一入口：解析 → comptime 块求值。
/// 返回（源码, 程序）。脚本展开已移除，见 `12-script-redesign.md`。
pub fn parse_with_scripts(source: &str) -> Result<(String, hc::Program), String> {
    let program = hc::parse_source(source).map_err(|d| diag::render(&d, source))?;
    // E1.2（组 D D2）：comptime 块装载期求值——语义检查前；
    // 失败 = 编译错误。IR/字节码/native 对 comptime 块无感知（后端跳过）。
    crate::comptime::eval_comptime_blocks(source, &program)?;
    Ok((source.to_string(), program))
}

/// 计算源码缓存键（hash 十六进制串）
/// 用于 `.hs` 文件缓存（B6-2）。
pub(crate) fn source_cache_key(source: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 脚本缓存目录：`~/.hc/cache/script/`（B6-2：保留供旧缓存清理）
pub(crate) fn cache_dir() -> PathBuf {
    home_subdir(&[".hc", "cache", "script"])
}

/// `.hs` 脚本缓存目录：`~/.hc/cache/hs/`（B6-2：.hs 脚本文件缓存）
pub(crate) fn hs_cache_dir() -> PathBuf {
    home_subdir(&[".hc", "cache", "hs"])
}

/// SDK 目录：`~/.hc/sdk/`（脚本文件引用搜索路径之一）
pub(crate) fn sdk_dir() -> PathBuf {
    home_subdir(&[".hc", "sdk"])
}

/// 计算 `~/.hc/` 下的子目录路径
fn home_subdir(parts: &[&str]) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let mut dir = PathBuf::from(home);
    for p in parts {
        dir.push(p);
    }
    dir
}

/// 尝试读取缓存；返回 None 表示缓存缺失或不可读
pub(crate) fn try_read_cache(path: &PathBuf) -> Option<String> {
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// 写入缓存（忽略失败——缓存非关键路径）
pub(crate) fn write_cache(path: &PathBuf, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, content);
}

/// M1-1：文件级命名空间自动推断。
/// 如果文件没有显式 `Decl::Namespace`，则根据 `ns_name` 包裹所有声明。
/// 一个文件只能有一个命名空间（设计决策 D1）。
pub fn infer_namespace(program: &mut Program, ns_name: &str) {
    let has_explicit = program
        .decls
        .iter()
        .any(|d| matches!(d, Decl::Namespace { .. }));
    if has_explicit {
        return;
    }
    let decls = std::mem::take(&mut program.decls);
    program.decls = vec![Decl::Namespace {
        name: ns_name.to_string(),
        decls,
        pub_: true,
        is_module: false,
        span: Span::new(0, 0, 0, 0),
    }];
}

/// 计算文件级命名空间名称。
/// 项目根目录（含 build.zon）存在时，命名空间 = 项目名 + 相对目录路径段（`/` → `.`）。
/// 无项目根目录时，命名空间 = 文件基本名。
/// 每个段首字母大写（PascalCase 命名规范）。
pub fn compute_namespace_name(file_path: &Path, project_root: Option<&Path>) -> String {
    if let Some(root) = project_root {
        if let Ok(rel) = file_path.strip_prefix(root) {
            let mut parts = Vec::new();
            let project_name = capitalize_first(&get_project_name_from_root(root));
            parts.push(project_name);
            if let Some(parent) = rel.parent() {
                if parent != Path::new("") {
                    for segment in parent {
                        parts.push(capitalize_first(segment.to_string_lossy().as_ref()));
                    }
                }
            }
            return parts.join(".");
        }
    }
    // 无项目根目录或文件不在项目根目录下：使用文件基本名
    capitalize_first(
        &file_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
    )
}

/// 首字母大写（PascalCase 命名规范辅助）
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// 从项目根目录读取项目名称（build.zon 中的 `name` 字段）。
/// 回退：使用目录名。
fn get_project_name_from_root(project_root: &Path) -> String {
    crate::project::buildzon::load_from_dir(project_root)
        .ok()
        .flatten()
        .map(|m| m.name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            project_root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
}

/// 从文件路径向上搜索 build.zon，返回项目根目录。
/// 搜索终止于根目录（无父目录时停止）。
pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        if dir.join("build.zon").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// 计算 `.hs` 脚本文件缓存键：基于文件路径 hash + 修改时间
/// 文件内容变更后 mtime 不同，自动失效
pub(crate) fn hs_cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let duration = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    let nanos = duration.as_nanos();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    nanos.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}
