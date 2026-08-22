//! Parser 声明解析：fn / global / const / trait 标注。

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::TokenKind;

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
                // 路径：`pkg.mod` / `H.std`（可含多段）；符号选择 `.{` 前止步
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
                if is_export {
                    return Err(
                        self.error_at("`export` only applies to `fn`/`async fn` declarations (K5)")
                    );
                }
                // E1（ADR-0013）：script 块——解析为声明级占位，装载期求值替换。
                // `close_end` = 块闭合 `}` 之后字节偏移：`parse_block` 消费 `}` 后 pos 指向
                // 其后 token（EOF 恒为末 token 哨兵），故 `tokens[pos-1]` 即 `}` 本身。
                self.advance();
                let body = self.parse_block()?;
                let close_end = self.tokens[self.pos - 1].span.end;
                let end = self.span();
                Ok(Decl::Script {
                    body,
                    close_end,
                    span: start.merge(&end),
                })
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
        let tr = match name.as_str() {
            "continuous" => Trait::Continuous,
            "pad" => Trait::Pad,
            "module" => Trait::Module,
            "align" => {
                self.expect(&TokenKind::LParen, "`(` after align")?;
                let t = self.parse_type()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                // 存储类型名（供布局计算 scalar_size 使用），非 Debug 字符串
                Trait::Align(match &t {
                    Type::Named(n, _) => n.clone(),
                    other => format!("{:?}", other),
                })
            }
            "test" => {
                // [test("名称")]：单参 = 测试显示名（可省，省略时显示函数名）
                let mut name = None;
                if self.at(&TokenKind::LParen) {
                    self.advance();
                    if let TokenKind::Str(first) = self.peek().clone() {
                        self.advance();
                        name = Some(first);
                    }
                    self.expect(&TokenKind::RParen, "`)`")?;
                }
                Trait::Test { name }
            }
            _ => {
                return Err(Diagnostic::error(
                    start,
                    format!("unknown trait attribute `[{name}]`"),
                ))
            }
        };
        self.expect(&TokenKind::RBracket, "`]`")?;
        Ok(Some(tr))
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
        let (is_test, test_name) = traits
            .iter()
            .find_map(|t| match t {
                Trait::Test { name } => Some((true, name.clone())),
                _ => None,
            })
            .unwrap_or((false, None));
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
            pub_: is_pub,
            is_async,
            exported: is_export,
            is_extern: false,
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
        let (is_test, test_name) = traits
            .iter()
            .find_map(|t| match t {
                Trait::Test { name } => Some((true, name.clone())),
                _ => None,
            })
            .unwrap_or((false, None));
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
            pub_: is_pub,
            is_async: false,
            exported: false,
            is_extern: true,
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
