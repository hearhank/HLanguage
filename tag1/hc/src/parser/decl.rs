//! 声明解析：函数、变量、常量、全局、类、枚举、结构体、接口、命名空间等声明

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::parser::test_attribute::TestExt;
use crate::semantic::trait_registry::TraitRegistry;
use crate::token::TokenKind;

// ---------- 特性处理器函数（Q24：字典式查找注册） ----------

fn parse_extension_trait(p: &mut Parser) -> Result<Trait, Diagnostic> {
    // [Extension(TypeName)]：解析类型名
    p.expect(&TokenKind::LParen, "`(` after Extension")?;
    let ty = p.expect_ident()?;
    p.expect(&TokenKind::RParen, "`)")?;
    Ok(Trait::Extension(ty))
}

fn parse_pad_trait(_p: &mut Parser) -> Result<Trait, Diagnostic> {
    Ok(Trait::Pad)
}

fn parse_module_trait(p: &mut Parser) -> Result<Trait, Diagnostic> {
    Err(p.error_at("[module] is removed. Use `src/Modules/` directory instead (see ADR-0026)."))
}

fn parse_align_trait(p: &mut Parser) -> Result<Trait, Diagnostic> {
    p.expect(&TokenKind::LParen, "`(` after align")?;
    let n = match p.peek().clone() {
        TokenKind::Int(ref s) => {
            let val = s
                .trim_end_matches(|c: char| c.is_alphabetic())
                .replace('_', "")
                .parse::<u32>()
                .map_err(|_| p.error_at(format!("invalid alignment value `{s}")))?;
            p.advance();
            val
        }
        _ => return Err(p.error_at("expected integer alignment value (1, 2, 4, or 8)")),
    };
    p.expect(&TokenKind::RParen, "`)")?;
    Ok(Trait::Align(n))
}

fn parse_test_trait(p: &mut Parser) -> Result<Trait, Diagnostic> {
    p.parse_test_attr()
}

/// 注册系统特性处理器到注册表
pub(crate) fn register_system_trait_handlers(reg: &mut TraitRegistry) {
    reg.register_handler("pad", parse_pad_trait);
    reg.register_handler("module", parse_module_trait);
    reg.register_handler("align", parse_align_trait);
    reg.register_handler("test", parse_test_trait);
    reg.register_handler("extension", parse_extension_trait);
}

impl Parser {
    pub(crate) fn parse_decl(&mut self) -> Result<Decl, Diagnostic> {
        // 可见性标注 pub（Q3/M7.2：跨包导出标志，默认私有）
        let is_pub = if self.at(&TokenKind::KwPub) {
            self.advance();
            true
        } else {
            false
        };
        // K5（ADR-0014）：`export` 修饰符——原生符号级导出（链接器可见）；与 pub 正交，
        // 仅作用于 `fn`/`async fn`（其它声明前缀 export → 下方报错）。
        let is_export = if self.at(&TokenKind::KwExport) {
            self.advance();
            true
        } else {
            false
        };
        // 特性标注（仅 class 前）：[continuous] [pad] [align(T)]
        let mut traits = Vec::new();
        while self.at(&TokenKind::LBracket) {
            if let Some(t) = self.parse_trait()? {
                traits.push(t);
            }
        }

        let start = self.span();
        match self.peek().clone() {
            TokenKind::KwGlobal => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_global(start, is_pub)
            }
            TokenKind::KwConst => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_const(start, is_pub)
            }
            TokenKind::KwAsync => {
                // 组 E E1：`async fn`——调用点返回 `Future(R)`（R = 声明返回类型）
                self.advance();
                self.expect(&TokenKind::KwFn, "`fn` after `async`")?;
                self.finish_fn_decl(start, &traits, is_pub, true, is_export)
            }
            TokenKind::KwExtern => {
                // A1（ADR-0020）：`extern fn` 纯声明（无 body，链接期解析外部 C 符号）
                self.advance();
                self.parse_extern_fn_decl(start, &traits, is_pub)
            }
            TokenKind::KwFn => {
                self.advance();
                self.finish_fn_decl(start, &traits, is_pub, false, is_export)
            }
            TokenKind::KwStruct => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_struct(start, traits, is_pub)
            }
            TokenKind::KwClass | TokenKind::KwTree => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_class(start, traits, is_pub)
            }
            TokenKind::KwEnum => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_enum(start, is_pub)
            }
            TokenKind::KwUnion => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_union(start, is_pub)
            }
            TokenKind::KwInterface => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                self.parse_interface(start, is_pub)
            }
            TokenKind::KwNamespace => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::LBrace, "`{` after namespace name")?;
                let mut decls = Vec::new();
                while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
                    match self.parse_decl() {
                        Ok(d) => decls.push(d),
                        Err(e) => {
                            self.diags.push(e);
                            self.synchronize();
                        }
                    }
                }
                self.expect(&TokenKind::RBrace, "`}` to close namespace")?;
                let end = self.span();
                Ok(Decl::Namespace {
                    name,
                    decls,
                    pub_: is_pub,
                    // `[module]` 特性标注（A2b，2026-08-17）：模块 = 隔离的命名空间
                    is_module: traits.iter().any(|t| matches!(t, Trait::Module)),
                    span: start.merge(&end),
                })
            }
            TokenKind::KwUsing => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                let path = self.parse_path()?;
                let alias = if self.is_ident("as") {
                    self.advance();
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semi, "`;` after using")?;
                let end = self.span();
                Ok(Decl::Using {
                    path,
                    alias,
                    span: start.merge(&end),
                })
            }
            TokenKind::KwImport => {
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                self.advance();
                // 文件引用：`import "path/to/file.hc"`（B6-2：.hs 脚本用文件路径而非命名空间）
                if matches!(self.peek(), TokenKind::Str(_)) {
                    let path = match self.advance().kind {
                        TokenKind::Str(s) => s,
                        _ => unreachable!(),
                    };
                    let alias = if self.is_ident("as") {
                        self.advance();
                        Some(self.expect_ident()?)
                    } else {
                        None
                    };
                    self.expect(&TokenKind::Semi, "`;` after import")?;
                    let end = self.span();
                    return Ok(Decl::Include {
                        path,
                        alias,
                        span: start.merge(&end),
                    });
                }
                // 命名空间路径：`pkg.mod` / `H.std`（可含多段）；符号选择 `.{` 前止步
                let path = self.parse_import_path()?;
                // 符号选择：`.{sym, sym as alias}`（`.{` 后非标识符——parse_path 已消费到 `.`）
                let select = if self.at(&TokenKind::Dot) && self.peek_n(1) == &TokenKind::LBrace {
                    self.advance(); // .
                    self.advance(); // {
                    let mut syms = Vec::new();
                    loop {
                        let name = self.expect_ident()?;
                        let alias = if self.is_ident("as") {
                            self.advance();
                            Some(self.expect_ident()?)
                        } else {
                            None
                        };
                        syms.push((name, alias));
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                    self.expect(&TokenKind::RBrace, "`}` after import symbol selection")?;
                    Some(syms)
                } else {
                    None
                };
                // 整模块别名：`import pkg.mod as m;`
                let alias = if select.is_none() && self.is_ident("as") {
                    self.advance();
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semi, "`;` after import")?;
                let end = self.span();
                Ok(Decl::Import {
                    path,
                    alias,
                    select,
                    span: start.merge(&end),
                })
            }
            TokenKind::KwScript => {
                // 2026-08-23：`script { }` 块已从 `.hc` 中移除。
                // 脚本功能迁移到 `.hs` 文件，见 `docs/SPEC/phase3/12-script-redesign.md`。
                return Err(self.error_at(
                    "`script { }` 块已移除。请使用 `.hs` 脚本文件替代 (docs/SPEC/phase3/12-script-redesign.md)"
                ));
            }
            TokenKind::KwComptime => {
                // E1.2（组 D D2）：comptime 块——声明级占位，装载期受限 Interp 求值
                // （结果丢弃、失败 = 编译错误）。不替换源码，无需 close_end。
                self.advance();
                let body = self.parse_block()?;
                let end = self.span();
                Ok(Decl::Comptime {
                    body,
                    span: start.merge(&end),
                })
            }
            other => Err(Diagnostic::error(
                start,
                format!("expected declaration, found {}", other.describe()),
            )),
        }
    }

    pub(crate) fn parse_trait(&mut self) -> Result<Option<Trait>, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::LBracket, "`[`")?;
        let name = self.expect_ident()?;
        let name_lower = name.to_lowercase();

        // 支持 struct 字面量语法：`[name{field=value, ...}]`
        if self.at(&TokenKind::LBrace) {
            let tr = self.parse_trait_struct_literal(&name_lower, start)?;
            self.expect(&TokenKind::RBracket, "`]`")?;
            return Ok(Some(tr));
        }

        // 旧语法：`[name]` 或 `[name(...)]`
        let tr = match self.trait_registry.lookup_handler(&name_lower) {
            Some(handler) => handler(self)?,
            None => {
                let known = self.trait_registry.known_names().join(", ");
                return Err(Diagnostic::error(
                    start,
                    format!("unknown trait attribute `[{name}]`; known traits: {known}"),
                ));
            }
        };
        self.expect(&TokenKind::RBracket, "`]`")?;
        Ok(Some(tr))
    }

    /// 解析 struct 字面量语法特性：`[name{field=value, ...}]`
    /// 将 struct 字面量转换为对应的 `Trait` 枚举值
    fn parse_trait_struct_literal(
        &mut self,
        name: &str,
        _start: Span,
    ) -> Result<Trait, Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Eq, "`=` in struct field")?;
            // 值可以是标识符、关键字（async/thread）或表达式
            let fval = if self.at(&TokenKind::KwAsync) {
                self.advance();
                Expr::Ident("async".to_string(), self.span())
            } else if self.is_ident("thread") {
                let s = self.expect_ident()?;
                Expr::Ident(s, self.span())
            } else {
                self.parse_expr()?
            };
            fields.push((fname, fval));
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "`}`")?;

        // 按名称分发到对应特性类型
        match name {
            "test" => self.build_test_from_attr(fields),
            "align" => self.build_align_from_struct(fields),
            _ => {
                let known = self.trait_registry.known_names().join(", ");
                Err(Diagnostic::error(
                    _start,
                    format!("unknown trait attribute `[{name}]`; known traits: {known}"),
                ))
            }
        }
    }

    /// 从 struct 字面量构建 `[align{value=N}]` 特性
    fn build_align_from_struct(&self, fields: Vec<(String, Expr)>) -> Result<Trait, Diagnostic> {
        if fields.len() != 1 {
            return Err(self.error_at("align requires exactly one field `value`"));
        }
        let (fname, fval) = &fields[0];
        if fname != "value" {
            return Err(self.error_at("align field must be `value`"));
        }
        if let Expr::IntLit { text, .. } = fval {
            let n = text
                .trim_end_matches(|c: char| c.is_alphabetic())
                .replace('_', "")
                .parse::<u32>()
                .map_err(|_| self.error_at(format!("invalid alignment value `{text}`")))?;
            Ok(Trait::Align(n))
        } else {
            Err(self.error_at("align.value must be an integer (1, 2, 4, or 8)"))
        }
    }

    pub(crate) fn parse_global(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        let ty = if self.at(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let init = if self.at(&TokenKind::Eq) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semi, "`;` after global declaration")?;
        Ok(Decl::Global {
            name,
            ty,
            init,
            pub_: is_pub,
            span: start,
        })
    }

    pub(crate) fn parse_const(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        let ty = if self.at(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq, "`=` in const declaration")?;
        // 错误集字面量 error{ ... } → 视为类型别名（tag1：解析为 VoidLit + 注册）
        if self.is_ident("error") && self.peek_n(1) == &TokenKind::LBrace {
            self.advance(); // error
            self.advance(); // {
            let mut names = Vec::new();
            while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
                names.push(self.expect_ident()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                }
            }
            self.expect(&TokenKind::RBrace, "`}` to close error set")?;
            self.expect(&TokenKind::Semi, "`;` after const")?;
            let span = start.merge(&self.span());
            return Ok(Decl::Const {
                name,
                ty: Some(Type::Named(
                    format!("error_set:{}", names.join(",")),
                    vec![],
                )),
                init: Expr::VoidLit(span.clone()),
                pub_: is_pub,
                span,
            });
        }
        // 错误集联合 A || B → 类型别名（Zig 式）
        if let TokenKind::Ident(first) = self.peek().clone() {
            if self.peek_n(1) == &TokenKind::PipePipe {
                let mut parts = vec![first];
                while matches!(self.peek(), TokenKind::Ident(_)) {
                    let _ = self.peek().clone();
                    if self.peek_n(1) != &TokenKind::PipePipe {
                        break;
                    }
                    self.advance(); // ident
                    self.advance(); // ||
                    if let TokenKind::Ident(n) = self.peek().clone() {
                        parts.push(n);
                    }
                }
                if let TokenKind::Ident(last) = self.peek().clone() {
                    parts.push(last);
                    self.advance();
                }
                self.expect(&TokenKind::Semi, "`;` after const")?;
                let span = start.merge(&self.span());
                return Ok(Decl::Const {
                    name,
                    ty: Some(Type::Named(
                        format!("error_set:{}", parts.join(",")),
                        vec![],
                    )),
                    init: Expr::VoidLit(span.clone()),
                    pub_: is_pub,
                    span,
                });
            }
        }
        let init = self.parse_expr()?;
        self.expect(&TokenKind::Semi, "`;` after const declaration")?;
        Ok(Decl::Const {
            name,
            ty,
            init,
            pub_: is_pub,
            span: start,
        })
    }

    /// 组 E E1：`fn`/`async fn` 共用的声明收尾——解析名/参/返回/where/体后构造 `Decl::Fn`。
    /// is_async = true 时调用点返回 `Future(R)`（语义层按 FnSig.is_async 包装）。
    /// is_export = true 时标记 K5 原生符号导出（链接器可见 thunk）。
    pub(crate) fn finish_fn_decl(
        &mut self,
        start: Span,
        traits: &[Trait],
        is_pub: bool,
        is_async: bool,
        is_export: bool,
    ) -> Result<Decl, Diagnostic> {
        let (name, type_params, params, ret, where_clause, body, span) =
            self.parse_fn_rest(start)?;
        let (is_test, test_name, test_mode, test_timeout) = traits
            .iter()
            .find_map(|t| match t {
                Trait::Test {
                    name,
                    mode,
                    timeout,
                } => Some((true, name.clone(), *mode, *timeout)),
                _ => None,
            })
            .unwrap_or((false, None, TestMode::Serial, None));
        let extension_of = traits.iter().find_map(|t| match t {
            Trait::Extension(ty) => Some(ty.clone()),
            _ => None,
        });
        Ok(Decl::Fn {
            name,
            type_params,
            params,
            ret,
            where_clause,
            body,
            span,
            is_test,
            test_name,
            test_mode,
            test_timeout,
            pub_: is_pub,
            is_async,
            exported: is_export,
            is_extern: false,
            extension_of,
        })
    }

    /// A1：`extern fn` 声明——纯声明（无 body，链接期解析外部 C 符号）
    pub(crate) fn parse_extern_fn_decl(
        &mut self,
        start: Span,
        traits: &[Trait],
        is_pub: bool,
    ) -> Result<Decl, Diagnostic> {
        self.expect(&TokenKind::KwFn, "`fn` after `extern`")?;
        let name = self.expect_name_or_keyword()?;
        let mut type_params: Vec<String> = Vec::new();
        if self.at(&TokenKind::Lt) {
            self.advance();
            loop {
                let tn = self.expect_ident()?;
                type_params.push(tn);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect_gt_generic()?;
        }
        self.expect(&TokenKind::LParen, "`(` after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "`)` after parameters")?;
        let ret = if !self.at(&TokenKind::Semi) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semi, "`;` after extern fn declaration")?;
        let end = self.span();
        let (is_test, test_name, test_mode, test_timeout) = traits
            .iter()
            .find_map(|t| match t {
                Trait::Test {
                    name,
                    mode,
                    timeout,
                } => Some((true, name.clone(), *mode, *timeout)),
                _ => None,
            })
            .unwrap_or((false, None, TestMode::Serial, None));
        Ok(Decl::Fn {
            name,
            type_params,
            params,
            ret,
            where_clause: Vec::new(),
            body: Block {
                stmts: vec![],
                span: end.clone(),
            },
            span: start.merge(&end),
            is_test,
            test_name,
            test_mode,
            test_timeout,
            pub_: is_pub,
            is_async: false,
            exported: false,
            is_extern: true,
            extension_of: None,
        })
    }

    pub(crate) fn parse_fn_rest(
        &mut self,
        start: Span,
    ) -> Result<
        (
            String,
            Vec<String>,
            Vec<Param>,
            Option<Type>,
            Vec<(String, Type)>,
            Block,
            Span,
        ),
        Diagnostic,
    > {
        let name = self.expect_name_or_keyword()?;
        // 泛型参数表：`fn swap<T>(...)` / `fn swap<T, U>(...)`。声明类型参数名；
        // 约束仍走 where 子句（M2.2）。`<T: type>` 型约束（comptime）暂不在此表。
        let mut type_params: Vec<String> = Vec::new();
        if self.at(&TokenKind::Lt) {
            self.advance();
            loop {
                let tn = self.expect_ident()?;
                type_params.push(tn);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect_gt_generic()?;
        }
        self.expect(&TokenKind::LParen, "`(` after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "`)` after parameters")?;
        let ret = if !self.at(&TokenKind::LBrace) {
            Some(self.parse_type()?)
        } else {
            None
        };
        // where 子句（M2.2：泛型约束保存，供语义检查器验证调用点约束）
        let mut where_clause: Vec<(String, Type)> = Vec::new();
        if self.at(&TokenKind::KwWhere) {
            self.advance();
            // 方法名恰为 `where` 时此处不是约束子句
            if name == "where" && !self.at(&TokenKind::Colon) {
                // 回退：已消费 where 关键字本身（函数名）；直接返回
            } else {
                loop {
                    let tn = self.expect_ident()?;
                    self.expect(&TokenKind::Colon, "`:` in where clause")?;
                    let iface = self.parse_type()?;
                    where_clause.push((tn, iface));
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }
        // 组 D：返回 `type` 的函数体按类型函数解析——`return [n]T;` 的 `[` 走
        // 数组类型值表达式（`Expr::ArrayType`），长度/元素可为 comptime 参数引用。
        let was_type_fn = self.in_type_fn;
        self.in_type_fn = matches!(
            ret.as_ref().map(|t| t.strip()),
            Some(Type::Named(n, _)) if n == "type"
        );
        let body = self.parse_block()?;
        self.in_type_fn = was_type_fn;
        let end = self.span();
        Ok((
            name,
            type_params,
            params,
            ret,
            where_clause,
            body,
            start.merge(&end),
        ))
    }

    pub(crate) fn parse_params(&mut self) -> Result<Vec<Param>, Diagnostic> {
        let mut params = Vec::new();
        if self.at(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let start = self.span();
            // 可选 var mut 前缀（如 var mut out: Vec<u8>）
            let mut_ = if self.at(&TokenKind::KwVar) {
                self.advance();
                if self.at(&TokenKind::KwMut) {
                    self.advance();
                    true
                } else {
                    false
                }
            } else {
                false
            };
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "`:` after parameter name")?;
            let ty = self.parse_type()?;
            let default = if self.at(&TokenKind::Eq) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                ty,
                default,
                span: start,
                mut_,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
                if self.at(&TokenKind::RParen) {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(params)
    }
}
