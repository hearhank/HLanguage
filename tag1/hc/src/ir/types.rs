//! IR 类型系统：IrType 类型表示与类型表构建

use super::*;

/// 由 `program.decls` 构建类型表（lower 阶段判型用；运行时类型名内嵌于值）。
pub(crate) fn build_type_table(program: &Program) -> TypeTable {
    let mut tt = TypeTable::default();
    collect_types(&program.decls, &mut tt, &[]);
    tt
}

pub(crate) fn collect_types(decls: &[Decl], tt: &mut TypeTable, path: &[String]) {
    for d in decls {
        match d {
            Decl::Class {
                name,
                traits,
                fields,
                methods,
                ..
            } => {
                let ci = ClassInfo {
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                    methods: methods.iter().map(|m| m.name.clone()).collect(),
                    continuous: false,
                };
                tt.classes.insert(name.clone(), ci);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.classes.insert(q, tt.classes[name].clone());
                }
            }
            Decl::Struct { name, fields, .. } => {
                let ci = ClassInfo {
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                    methods: vec![],
                    continuous: true,
                };
                tt.classes.insert(name.clone(), ci);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.classes.insert(q, tt.classes[name].clone());
                }
            }
            Decl::Enum { name, variants, .. } => {
                let ei = EnumInfo {
                    variants: variants.iter().map(|v| v.name.clone()).collect(),
                };
                tt.enums.insert(name.clone(), ei);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.enums.insert(q, tt.enums[name].clone());
                }
            }
            // K1 无标签 union（ADR-0014）：登记字段声明（扁平 + 全限定）
            Decl::Union { name, fields, .. } => {
                let ui = UnionInfo {
                    fields: fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                };
                tt.unions.insert(name.clone(), ui);
                if !path.is_empty() {
                    let mut q = path.join(".");
                    q.push('.');
                    q.push_str(name);
                    tt.unions.insert(q, tt.unions[name].clone());
                }
            }
            Decl::Namespace { name, decls, .. } => {
                tt.namespaces.insert(name.clone());
                let mut p = path.to_vec();
                p.push(name.clone());
                collect_types(decls, tt, &p);
            }
            _ => {}
        }
    }
}

// ---------- AST → IR 降级 ----------
