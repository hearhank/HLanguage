//! E1.1（组 B，ADR-0013）：`script { }` 块装载期展开。
//!
//! 流程（ADR-0013 决策 1）：解析 → 求值首个 `script { }` 块（受限 Interp，
//! io/alloc/argv/网络不可用，注入 `types` 元数据对象）→ 产物字符串**替换该块
//! 文本区间** → 重解析，循环直至无 script 块。产物须为字符串；任何阶段失败 =
//! 编译错误（带块内 + 所属块位置，经 `diag::render` 渲染）。
//!
//! 展开在装载期完成，IR/字节码/native 对展开后的 AST 无感知（ADR-0013 决策 3、
//! 组 B5 三后端零改动）。

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use hc::ast::{Block, Decl, Program};
use hc::diag;
use hc_rt::{Interp, Value};

/// 最大展开轮数（防产物含自引用 script 块导致的无限循环）
const MAX_EXPANSION_ROUNDS: usize = 1000;

/// 展开源码中的全部 script 块，返回展开后源码。无 script 块时原样返回
/// （快速路径：文本不含 `script` 标识符即跳过解析）。
pub fn expand_scripts(source: &str) -> Result<String, String> {
    if !source.contains("script") {
        return Ok(source.to_string());
    }
    let mut cur = source.to_string();
    for _round in 0..MAX_EXPANSION_ROUNDS {
        let program = hc::parse_source(&cur).map_err(|d| diag::render(&d, &cur))?;
        let Some(site) = find_first_script(&program) else {
            return Ok(cur);
        };
        let product = eval_script(&cur, &program, &site)?;
        let mut out = String::with_capacity(cur.len() + product.len());
        out.push_str(&cur[..site.start]);
        out.push_str(&product);
        out.push_str(&cur[site.close_end..]);
        cur = out;
    }
    Err("script 展开超过最大轮数（疑似产物含自引用 script 块）".into())
}

/// 装载路径统一入口：script 展开 → comptime 块求值 → 解析。
/// 返回（展开后源码, 展开后程序）。展开/comptime 求值失败返回已渲染诊断文本（调用方直接打印）。
///
/// 注：内联 `script { }` 块不缓存（依赖 `types` 上下文，非纯函数）。
pub fn parse_with_scripts(source: &str) -> Result<(String, hc::Program), String> {
    // 快速路径：无 script 块时不展开
    if !source.contains("script") {
        let program = hc::parse_source(source).map_err(|d| diag::render(&d, source))?;
        crate::comptimegen::eval_comptime_blocks(source, &program)?;
        return Ok((source.to_string(), program));
    }
    let expanded = expand_scripts(source)?;
    let program = hc::parse_source(&expanded).map_err(|d| diag::render(&d, &expanded))?;
    // E1.2（组 D D2）：comptime 块装载期求值——script 展开后（可见生成类型）、
    // 语义检查前；失败 = 编译错误。IR/字节码/native 对 comptime 块无感知（后端跳过）。
    crate::comptimegen::eval_comptime_blocks(&expanded, &program)?;
    Ok((expanded, program))
}

/// 计算源码缓存键（hash 十六进制串）
/// 用于 `.hs` 文件缓存（见 `run_hs_file`）。
#[allow(dead_code)]
pub(crate) fn source_cache_key(source: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 脚本缓存目录：`~/.hc/cache/script/`
#[allow(dead_code)]
pub(crate) fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let mut dir = PathBuf::from(home);
    dir.push(".hc");
    dir.push("cache");
    dir.push("script");
    dir
}

/// 尝试读取缓存；返回 None 表示缓存缺失或不可读
#[allow(dead_code)]
pub(crate) fn try_read_cache(path: &PathBuf) -> Option<String> {
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// 写入缓存（忽略失败——缓存非关键路径）
#[allow(dead_code)]
pub(crate) fn write_cache(path: &PathBuf, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, content);
}

/// 待展开的 script 块站点（源码文本区间锚）
struct ScriptSite<'a> {
    body: &'a Block,
    /// `script` 关键字起始字节偏移
    start: usize,
    /// 块闭合 `}` 之后字节偏移（替换终点，parser 精确捕获）
    close_end: usize,
}

/// 按源码序查找首个 script 块（顶层 + 命名空间体递归；class 内无 script 声明）
fn find_first_script(program: &Program) -> Option<ScriptSite<'_>> {
    for d in &program.decls {
        if let Some(s) = find_in_decl(d) {
            return Some(s);
        }
    }
    None
}

fn find_in_decl<'a>(d: &'a Decl) -> Option<ScriptSite<'a>> {
    match d {
        Decl::Script {
            body,
            close_end,
            span,
        } => Some(ScriptSite {
            body,
            start: span.start,
            close_end: *close_end,
        }),
        Decl::Namespace { decls, .. } => decls.iter().find_map(find_in_decl),
        _ => None,
    }
}

/// 求值 script 块体（受限 Interp）→ 产物字符串。
/// 失败 = 编译错误（已渲染诊断）；产物非字符串 = 编译错误。
fn eval_script(source: &str, program: &Program, site: &ScriptSite) -> Result<String, String> {
    let mut interp = Interp::new(source);
    interp.set_script_mode(true);
    interp
        .load(program)
        .map_err(|e| format!("script 块装载失败: {}", e.render(source)))?;
    let v = interp
        .exec_fn_body(site.body, &[])
        .map_err(|e| format!("script 块求值失败: {}", e.render(source)))?;
    match v {
        Value::Str(s) => Ok(String::from_utf8_lossy(&s.borrow()).into_owned()),
        other => Err(format!(
            "script 块求值结果必须是字符串（产物 = 代码字符串就地替换），得到 {}",
            value_kind(&other)
        )),
    }
}

/// 值的简短中文描述（script 产物非字符串错误提示用）
fn value_kind(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "整数",
        Value::Float(_) => "浮点",
        Value::Bool(_) => "布尔",
        Value::Str(_) => "字符串",
        Value::Arr(_) => "数组",
        Value::Slice { .. } => "切片",
        Value::Class(_) => "class 实例",
        Value::Enum { .. } => "枚举变体",
        Value::Opt(_) => "可选值",
        Value::Err { .. } => "错误值",
        Value::Ptr(_) => "指针",
        Value::Boxed(_) => "装箱值",
        Value::Vec(_) => "Vec",
        Value::Map(_) => "Map",
        Value::Alloc => "alloc",
        _ => "值",
    }
}
