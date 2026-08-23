//! 类型解析：AST Type → 静态类型 + 成员/字段/长度查询。

use super::*;
use crate::ast::*;

impl Checker {
    // ---------- 类型解析 ----------

    /// AST Type → 静态类型（大写未登记标识符 → 泛型参数，与运行时启发式一致）
    pub(crate) fn ty_of(&self, t: &Type) -> SType {
        match t.strip() {
            Type::Named(n, args) => {
                match n.as_str() {
                    "i8" => {
                        return SType::Int {
                            width: IntWidth::I8,
                        }
                    }
                    "i16" => {
                        return SType::Int {
                            width: IntWidth::I16,
                        }
                    }
                    "i32" => {
                        return SType::Int {
                            width: IntWidth::I32,
                        }
                    }
                    "i64" => {
                        return SType::Int {
                            width: IntWidth::I64,
                        }
                    }
                    "i128" => {
                        return SType::Int {
                            width: IntWidth::I128,
                        }
                    }
                    "isize" => {
                        return SType::Int {
                            width: IntWidth::ISize,
                        }
                    }
                    "u8" => {
                        return SType::Int {
                            width: IntWidth::U8,
                        }
                    }
                    "u16" => {
                        return SType::Int {
                            width: IntWidth::U16,
                        }
                    }
                    "u32" => {
                        return SType::Int {
                            width: IntWidth::U32,
                        }
                    }
                    "u64" => {
                        return SType::Int {
                            width: IntWidth::U64,
                        }
                    }
                    "u128" => {
                        return SType::Int {
                            width: IntWidth::U128,
                        }
                    }
                    "usize" => {
                        return SType::Int {
                            width: IntWidth::USize,
                        }
                    }
                    // comptime_int（组 D D4）：惰性宽度整数——定型点收窄，Comptime 宽度跳过收窄检查
                    "comptime_int" => {
                        return SType::Int {
                            width: IntWidth::Comptime,
                        }
                    }
                    // comptime_float（组 D D4）：惰性宽度浮点——H 浮点单一 f64 表示，映射 SType::Float
                    "comptime_float" => return SType::Float,
                    "f16" | "f32" | "f64" | "f128" => return SType::Float,
                    "bool" => return SType::Bool,
                    "void" => return SType::Void,
                    "String" => return SType::Str,
                    "Allocator" | "ExitType" => return SType::Named(n.clone(), vec![]),
                    // 组 E E1：Future(R)——async fn 调用返回类型；await 解包取 R
                    "Future" => {
                        return SType::Named(
                            n.clone(),
                            args.iter().map(|a| self.ty_of(a)).collect(),
                        )
                    }
                    _ => {}
                }
                if is_builtin_type(n) {
                    return SType::Named(n.clone(), args.iter().map(|a| self.ty_of(a)).collect());
                }
                match self.types.get(n) {
                    Some(_) => {
                        SType::Named(n.clone(), args.iter().map(|a| self.ty_of(a)).collect())
                    }
                    None => {
                        // 大写未登记 → 泛型参数（启发式）；小写未登记 → 未知
                        if n.chars().next().map_or(false, |c| c.is_uppercase()) {
                            SType::Generic(n.clone())
                        } else {
                            SType::Unknown
                        }
                    }
                }
            }
            Type::Ptr(inner, mut_) => SType::Ptr(Box::new(self.ty_of(inner)), *mut_),
            Type::Slice(inner, _) => SType::Slice(Box::new(self.ty_of(inner))),
            Type::Optional(inner) => SType::Optional(Box::new(self.ty_of(inner))),
            Type::ErrorUnion(e, inner) => SType::ErrorUnion(
                e.as_ref().map(|x| Box::new(self.ty_of(x))),
                Box::new(self.ty_of(inner)),
            ),
            Type::Tuple(ts) => SType::Tuple(ts.iter().map(|x| self.ty_of(x)).collect()),
            Type::Array(n, inner) => SType::Array(*n, Box::new(self.ty_of(inner))),
            // comptime_int 字面量（组 D）：惰性宽度整数——定型点收窄，此处记宽度
            Type::ComptimeInt(_) => SType::Int {
                width: IntWidth::Comptime,
            },
            Type::Infer => SType::Infer,
            Type::Owned(inner) => self.ty_of(inner),
        }
    }

    /// 成员访问/索引前自动解引用（评审 A3：p.x、s[i]）
    pub(crate) fn deref_member<'a>(&self, t: &'a SType) -> &'a SType {
        match t {
            SType::Ptr(inner, _) => inner,
            other => other,
        }
    }

    /// 容器/切片/字符串的 `.len` 字段 → usize
    pub(crate) fn len_field_ty(&self, t: &SType) -> Option<SType> {
        match t {
            SType::Slice(_) | SType::Str | SType::Array(_, _) => Some(SType::Int {
                width: IntWidth::USize,
            }),
            SType::Named(n, _) if is_collection(n) => Some(SType::Int {
                width: IntWidth::USize,
            }),
            _ => None,
        }
    }

    /// 类字段类型查询（已解引用；K1 union 字段同走此路径——字段读取经字节重解释）
    pub(crate) fn class_field_ty(&self, t: &SType, field: &str) -> Option<SType> {
        match t {
            SType::Named(cn, _) => {
                if let Some(TypeKind::Class { fields, .. }) = self.types.get(cn).map(|i| &i.kind) {
                    if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                        return Some(self.ty_of(&fd.ty));
                    }
                }
                if let Some(TypeKind::Struct { fields, .. }) = self.types.get(cn).map(|i| &i.kind) {
                    if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                        return Some(self.ty_of(&fd.ty));
                    }
                }
                if let Some(TypeKind::Union { fields, .. }) = self.types.get(cn).map(|i| &i.kind) {
                    if let Some(fd) = fields.iter().find(|f| f.name == *field) {
                        return Some(self.ty_of(&fd.ty));
                    }
                }
                None
            }
            _ => None,
        }
    }
}
