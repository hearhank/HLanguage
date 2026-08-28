//! 导入收集（用于 unused_import）

use hc::ast::*;
use hc::token::Span;

pub(crate) fn collect_imports(program: &Program) -> Vec<(String, Span)> {
    let mut imports = Vec::new();
    for d in &program.decls {
        if let Decl::Import {
            path,
            alias,
            select,
            span,
        } = d
        {
            // 导入的名称为路径末段（或别名）
            let name = alias
                .clone()
                .unwrap_or_else(|| path.last().cloned().unwrap_or_default());
            // 符号选择导入：每个符号名单独算
            if let Some(sel) = select {
                for (orig, _alias) in sel {
                    let import_name = _alias.clone().unwrap_or_else(|| orig.clone());
                    imports.push((import_name, span.clone()));
                }
            } else {
                imports.push((name, span.clone()));
            }
        }
    }
    imports
}
