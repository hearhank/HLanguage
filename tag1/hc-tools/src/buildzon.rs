//! M7.2：build.zon 清单解析——依赖清单 = H 数据字面量（`const build = Build{...}`）。
//!
//! 清单即数据：`hc pkg add` / 注册中心 / 指纹校验归第三块 E5；tag1 仅解析
//! 本地依赖（`Pkg{ ..., path = "../x" }`）；无 path 视为注册中心依赖（跳过）。

use std::path::{Path, PathBuf};

use hc::ast::{Decl, Expr};

/// 包清单（build.zon 的 `Build{...}`）
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub kind: Kind,
    pub files: Vec<String>,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Exe,
    Lib,
    Script,
}

/// 依赖项（`Pkg{ ... }`）——tag1：本地依赖靠 `path`；无 path 为注册中心依赖（跳过）
#[derive(Debug, Clone, PartialEq)]
pub struct Dep {
    pub name: String,
    pub version: String,
    pub fingerprint: Option<String>,
    pub path: Option<PathBuf>,
}

/// 解析 build.zon 源码为清单；失败返回可读错误文本
pub fn parse(src: &str) -> Result<Manifest, String> {
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
    let fields = match init {
        Expr::NamedLit { ty, fields, .. } if ty == "Build" => fields,
        _ => return Err("build.zon: `build` 必须是 `Build{ ... }` 数据字面量".into()),
    };

    let mut name = None;
    let mut version = None;
    let mut kind = Kind::Exe;
    let mut files = Vec::new();
    let mut deps = Vec::new();
    for (key, val) in fields {
        match key.as_str() {
            "name" => name = Some(expect_str(key, val)?),
            "version" => version = Some(expect_str(key, val)?),
            "kind" => kind = parse_kind(val)?,
            "files" => files = expect_str_array(key, val)?,
            "deps" => deps = parse_deps(val)?,
            _ => {} // 未知字段（作者/构建选项等）——tag1 忽略
        }
    }
    Ok(Manifest {
        name: name.unwrap_or_default(),
        version: version.unwrap_or_default(),
        kind,
        files,
        deps,
    })
}

/// 读取目录下的 build.zon（不存在则 Ok(None)）
pub fn load_from_dir(dir: &Path) -> Result<Option<Manifest>, String> {
    let p = dir.join("build.zon");
    if !p.exists() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
    parse(&src).map(Some)
}

fn expect_str(key: &str, e: &Expr) -> Result<String, String> {
    match e {
        Expr::StrLit { value, .. } => Ok(value.clone()),
        _ => Err(format!("build.zon: 字段 `{key}` 应为字符串字面量")),
    }
}

fn expect_str_array(key: &str, e: &Expr) -> Result<Vec<String>, String> {
    match e {
        Expr::ArrayLit(items, _) => items.iter().map(|i| expect_str(key, i)).collect(),
        _ => Err(format!("build.zon: 字段 `{key}` 应为字符串数组")),
    }
}

fn parse_kind(e: &Expr) -> Result<Kind, String> {
    match e {
        Expr::Dot { base, field, .. } => match (&**base, field.as_str()) {
            (Expr::Ident(n, _), "exe") if n == "Kind" => Ok(Kind::Exe),
            (Expr::Ident(n, _), "lib") if n == "Kind" => Ok(Kind::Lib),
            (Expr::Ident(n, _), "script") if n == "Kind" => Ok(Kind::Script),
            _ => Err(format!(
                "build.zon: 未知 kind `{field}`（应为 Kind.exe/lib/script）"
            )),
        },
        _ => Err("build.zon: 字段 `kind` 应为 Kind.exe/lib/script".into()),
    }
}

fn parse_deps(e: &Expr) -> Result<Vec<Dep>, String> {
    match e {
        Expr::ArrayLit(items, _) => items.iter().map(parse_dep).collect(),
        _ => Err("build.zon: 字段 `deps` 应为 Pkg 数组".into()),
    }
}

fn parse_dep(e: &Expr) -> Result<Dep, String> {
    let fields = match e {
        Expr::NamedLit { ty, fields, .. } if ty == "Pkg" => fields,
        _ => return Err("build.zon: 依赖项应为 `Pkg{ ... }`".into()),
    };
    let mut name = String::new();
    let mut version = String::new();
    let mut fingerprint = None;
    let mut path = None;
    for (key, val) in fields {
        match key.as_str() {
            "name" => name = expect_str(key, val)?,
            "version" => version = expect_str(key, val)?,
            "fingerprint" => fingerprint = Some(parse_fingerprint(val)?),
            "path" => path = Some(PathBuf::from(expect_str(key, val)?)),
            _ => {} // source/author 等——tag1 忽略
        }
    }
    if name.is_empty() {
        return Err("build.zon: 依赖项缺少 `name`".into());
    }
    Ok(Dep {
        name,
        version,
        fingerprint,
        path,
    })
}

fn parse_fingerprint(e: &Expr) -> Result<String, String> {
    match e {
        // 新旧兼容：整数字面量（旧格式 `0xa1b2`）→ 十六进制小写
        Expr::IntLit { text, .. } => {
            let (n, _) = hc_rt::parse_int_text(text)
                .map_err(|err| format!("build.zon: 指纹 `{text}` 非法: {}", err.message))?;
            Ok(format!("{:x}", n))
        }
        // 新格式：SHA-256 十六进制字符串
        Expr::StrLit { value, .. } => {
            if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("build.zon: 指纹应为 64 字符十六进制字符串（SHA-256）".into());
            }
            Ok(value.to_ascii_lowercase())
        }
        _ => Err("build.zon: 字段 `fingerprint` 应为 SHA-256 十六进制字符串或整数字面量".into()),
    }
}

#[cfg(test)]
mod tests;
