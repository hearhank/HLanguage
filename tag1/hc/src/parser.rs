//! Parser（M1.2）：token 流 → AST
//!
//! 递归下降 + 运算符优先级表（Q4 定案）：
//! 后缀 > 前缀/一元 > `*` `/` `%` `%%` > `+` `-` > `<<` `>>` > `&` > `^` > `|` > 比较（非结合）> and/or

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::{Span, Token, TokenKind};

pub type ParseResult<T> = Result<T, Vec<Diagnostic>>;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
}

impl Parser {
    pub fn new(_source: &str, tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diags: Vec::new(),
        }
    }

    pub fn parse_program(mut self) -> ParseResult<Program> {
        let mut decls = Vec::new();
        while !self.at(&TokenKind::Eof) {
            match self.parse_decl() {
                Ok(d) => decls.push(d),
                Err(e) => {
                    self.diags.push(e);
                    self.synchronize();
                }
            }
        }
        if self.diags.is_empty() {
            Ok(Program { decls })
        } else {
            Err(self.diags)
        }
    }

    // ---------- 基础工具 ----------

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }
    fn peek_n(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }
    fn span(&self) -> Span {
        self.tokens[self.pos].span.clone()
    }
    fn at(&self, k: &TokenKind) -> bool {
        self.peek() == k
    }
    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn expect(&mut self, k: &TokenKind, what: &str) -> Result<Token, Diagnostic> {
        if self.at(k) {
            Ok(self.advance())
        } else {
            Err(Diagnostic::error(
                self.span(),
                format!("expected {what}, found {}", self.peek().describe()),
            ))
        }
    }
    fn error_at(&self, msg: impl Into<String>) -> Diagnostic {
        Diagnostic::error(self.span(), msg)
    }
    fn synchronize(&mut self) {
        while !self.at(&TokenKind::Eof) {
            if matches!(
                self.peek(),
                TokenKind::KwFn
                    | TokenKind::KwClass
                    | TokenKind::KwEnum
                    | TokenKind::KwInterface
                    | TokenKind::KwNamespace
                    | TokenKind::KwUsing
                    | TokenKind::KwImport
                    | TokenKind::KwGlobal
            ) {
                return;
            }
            self.advance();
        }
    }

    // ---------- 声明 ----------

    fn parse_decl(&mut self) -> Result<Decl, Diagnostic> {
        // 可见性标注 pub（Q3/M7.2：跨包导出标志，默认私有）
        let is_pub = if self.at(&TokenKind::KwPub) {
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
                self.advance();
                self.parse_global(start, is_pub)
            }
            TokenKind::KwConst => {
                self.advance();
                self.parse_const(start, is_pub)
            }
            TokenKind::KwFn => {
                self.advance();
                let (name, params, ret, where_clause, body, span) = self.parse_fn_rest(start)?;
                let (is_test, test_name) = traits
                    .iter()
                    .find_map(|t| match t {
                        Trait::Test { name } => Some((true, name.clone())),
                        _ => None,
                    })
                    .unwrap_or((false, None));
                Ok(Decl::Fn {
                    name,
                    params,
                    ret,
                    where_clause,
                    body,
                    span,
                    is_test,
                    test_name,
                    pub_: is_pub,
                })
            }
            TokenKind::KwClass | TokenKind::KwTree => {
                self.advance();
                self.parse_class(start, traits, is_pub)
            }
            TokenKind::KwEnum => {
                self.advance();
                self.parse_enum(start, is_pub)
            }
            TokenKind::KwInterface => {
                self.advance();
                self.parse_interface(start, is_pub)
            }
            TokenKind::KwNamespace => {
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
            other => Err(Diagnostic::error(
                start,
                format!("expected declaration, found {}", other.describe()),
            )),
        }
    }

    fn parse_trait(&mut self) -> Result<Option<Trait>, Diagnostic> {
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

    fn parse_global(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
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

    fn parse_const(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
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

    fn parse_fn_rest(
        &mut self,
        start: Span,
    ) -> Result<
        (
            String,
            Vec<Param>,
            Option<Type>,
            Vec<(String, Type)>,
            Block,
            Span,
        ),
        Diagnostic,
    > {
        let name = self.expect_name_or_keyword()?;
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
        let body = self.parse_block()?;
        let end = self.span();
        Ok((name, params, ret, where_clause, body, start.merge(&end)))
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, Diagnostic> {
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

    fn parse_class(
        &mut self,
        start: Span,
        traits: Vec<Trait>,
        is_pub: bool,
    ) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        let mut ifaces = Vec::new();
        if self.at(&TokenKind::Colon) {
            self.advance();
            loop {
                ifaces.push(self.parse_type()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::LBrace, "`{` to open class body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            // Q3：成员可见性——方法默认公开；属性默认私有，`pub` 显式导出
            let member_pub = if self.at(&TokenKind::KwPub) {
                self.advance();
                true
            } else {
                false
            };
            if self.at(&TokenKind::KwFn) {
                self.advance();
                let mstart = self.span();
                let (mname, mparams, mret, mwhere, mbody, mspan) = self.parse_fn_rest(mstart)?;
                methods.push(Method {
                    name: mname,
                    params: mparams,
                    ret: mret,
                    where_clause: mwhere,
                    body: mbody,
                    span: mspan,
                });
            } else {
                // 字段：name: Type,（可带 mut 前缀——属性无所有权标注，Q3/H5）
                if self.at(&TokenKind::KwMut) {
                    self.advance();
                }
                let fstart = self.span();
                let fname = self.expect_ident()?;
                self.expect(&TokenKind::Colon, "`:` after field name")?;
                let fty = self.parse_type()?;
                fields.push(FieldDecl {
                    name: fname,
                    ty: fty,
                    pub_: member_pub,
                    span: fstart,
                });
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else if !self.at(&TokenKind::RBrace) {
                    return Err(self.error_at("expected `,` or `}` after class field"));
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close class body")?;
        let end = self.span();
        Ok(Decl::Class {
            name,
            ifaces,
            traits,
            fields,
            methods,
            pub_: is_pub,
            span: start.merge(&end),
        })
    }

    fn parse_enum(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "`{` to open enum body")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let vstart = self.span();
            // 变体名可为关键字（如 null）
            let vname = match self.peek() {
                TokenKind::Ident(s) => {
                    let s = s.clone();
                    self.advance();
                    s
                }
                TokenKind::KwNull => {
                    self.advance();
                    "null".to_string()
                }
                other => {
                    return Err(Diagnostic::error(
                        self.span(),
                        format!("expected enum variant name, found {}", other.describe()),
                    ))
                }
            };
            let payload = if self.at(&TokenKind::Colon) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            variants.push(EnumVariant {
                name: vname,
                payload,
                span: vstart,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else if !self.at(&TokenKind::RBrace) {
                return Err(self.error_at("expected `,` or `}` after enum variant"));
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close enum body")?;
        let end = self.span();
        Ok(Decl::Enum {
            name,
            variants,
            pub_: is_pub,
            span: start.merge(&end),
        })
    }

    /// 接口方法（tag1 已支持）；解析返回类型 `!void` 与 `where` 子句
    fn parse_interface_method(&mut self) -> Result<Method, Diagnostic> {
        self.advance(); // fn
        let mstart = self.span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen, "`(` after method name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "`)` after parameters")?;
        let ret = if !self.at(&TokenKind::Semi) && !self.at(&TokenKind::RBrace) {
            Some(self.parse_type()?)
        } else {
            None
        };
        // where 子句（接口方法：where T: Io）
        let mut where_clause: Vec<(String, Type)> = Vec::new();
        if self.at(&TokenKind::KwWhere) {
            self.advance();
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
        if self.at(&TokenKind::Semi) {
            self.advance();
        }
        let mspan = mstart.merge(&self.span());
        Ok(Method {
            name,
            params,
            ret,
            where_clause,
            body: Block {
                stmts: vec![],
                span: mspan.clone(),
            },
            span: mspan,
        })
    }

    fn parse_interface(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        let mut supers = Vec::new();
        if self.at(&TokenKind::Colon) {
            self.advance();
            loop {
                supers.push(self.parse_type()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::LBrace, "`{` to open interface body")?;
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::KwFn) {
                methods.push(self.parse_interface_method()?);
            } else {
                return Err(self.error_at("expected method declaration in interface"));
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close interface body")?;
        let end = self.span();
        Ok(Decl::Interface {
            name,
            supers,
            methods,
            pub_: is_pub,
            span: start.merge(&end),
        })
    }

    /// 跳过匿名 struct/class 字面量 `struct { ... }`（tag1：匿名类型归 E1）
    fn skip_anon_struct(&mut self) -> Result<(), Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::LBrace) {
                self.skip_anon_struct()?;
                continue;
            }
            if self.at(&TokenKind::Semi) {
                self.advance();
                continue;
            }
            self.advance();
        }
        self.expect(&TokenKind::RBrace, "`}`")?;
        Ok(())
    }

    fn parse_path(&mut self) -> Result<Vec<String>, Diagnostic> {
        let mut path = vec![self.expect_ident()?];
        while self.at(&TokenKind::Dot) {
            self.advance();
            path.push(self.expect_ident()?);
        }
        Ok(path)
    }

    /// import 路径：同 `parse_path`，但符号选择 `.{`（`.` 后跟 `{`）止步——
    /// `import H.std.{io as my};` 的 `H.std` 到 `.{` 为止。
    fn parse_import_path(&mut self) -> Result<Vec<String>, Diagnostic> {
        let mut path = vec![self.expect_ident()?];
        loop {
            if self.at(&TokenKind::Dot) && self.peek_n(1) == &TokenKind::LBrace {
                break; // 符号选择分隔符，留给 parse_decl 消费
            }
            if self.at(&TokenKind::Dot) {
                self.advance();
                path.push(self.expect_ident()?);
            } else {
                break;
            }
        }
        Ok(path)
    }

    // ---------- 类型 ----------

    pub fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        // o T（所有权形态）
        if self.at(&TokenKind::KwO) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Owned(Box::new(inner)));
        }
        // *T / *mut T
        if self.at(&TokenKind::Star) {
            self.advance();
            let mut_ = if self.at(&TokenKind::KwMut) {
                self.advance();
                true
            } else {
                false
            };
            let inner = self.parse_type()?;
            return Ok(Type::Ptr(Box::new(inner), mut_));
        }
        // &[T] / &mut [T] 或 &T（引用/切片；tag1：&Vec(X)、&i32 等引用形态）
        if self.at(&TokenKind::Amp) {
            self.advance();
            let mut_ = if self.at(&TokenKind::KwMut) {
                self.advance();
                true
            } else {
                false
            };
            // 切片 &[T]
            if self.at(&TokenKind::LBracket) {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(&TokenKind::RBracket, "`]` in slice type")?;
                return Ok(Type::Slice(Box::new(inner), mut_));
            }
            // 引用类型 &T（Vec 等）
            let inner = self.parse_type()?;
            return Ok(Type::Slice(Box::new(inner), mut_));
        }
        // ?T
        if self.at(&TokenKind::Question) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Optional(Box::new(inner)));
        }
        // !T（错误联合 anyerror!T）
        if self.at(&TokenKind::Bang) {
            self.advance();
            let t = self.parse_type()?;
            return Ok(Type::ErrorUnion(None, Box::new(t)));
        }
        // E!T（命名错误集）
        let base = self.parse_type_base()?;
        if self.at(&TokenKind::Bang) {
            self.advance();
            let t = self.parse_type()?;
            return Ok(Type::ErrorUnion(Some(Box::new(base)), Box::new(t)));
        }
        Ok(base)
    }

    fn parse_type_base(&mut self) -> Result<Type, Diagnostic> {
        // void 关键字
        if self.at(&TokenKind::KwVoid) {
            self.advance();
            return Ok(Type::Named("void".into(), vec![]));
        }
        // anytype：调用点推断（Q-S9）
        if self.at(&TokenKind::KwAnytype) {
            self.advance();
            return Ok(Type::Infer);
        }
        // type（类型即值，E1 完整；tag1 占位）
        if self.at(&TokenKind::KwType) {
            self.advance();
            return Ok(Type::Named("type".into(), vec![]));
        }
        // (T1, T2) 元组
        if self.at(&TokenKind::LParen) {
            self.advance();
            let mut items = vec![self.parse_type()?];
            while self.at(&TokenKind::Comma) {
                self.advance();
                items.push(self.parse_type()?);
            }
            self.expect(&TokenKind::RParen, "`)` to close tuple type")?;
            return Ok(Type::Tuple(items));
        }
        // [N]T 定长数组
        if self.at(&TokenKind::LBracket) {
            self.advance();
            let n = match self.peek() {
                TokenKind::Int(s) => {
                    let n = s
                        .trim_end_matches(|c: char| c.is_alphabetic())
                        .replace('_', "")
                        .parse::<usize>()
                        .map_err(|_| self.error_at(format!("invalid array length `{s}`")))?;
                    self.advance();
                    n
                }
                other => {
                    return Err(Diagnostic::error(
                        self.span(),
                        format!("expected array length, found {}", other.describe()),
                    ))
                }
            };
            self.expect(&TokenKind::RBracket, "`]` in array type")?;
            let inner = self.parse_type()?;
            return Ok(Type::Array(n, Box::new(inner)));
        }
        let mut name = self.expect_ident()?;
        // M1.4 限定类型名：Orders.Line（命名空间限定；双注册后按全名查找）
        while self.at(&TokenKind::Dot) {
            self.advance();
            let part = self.expect_name_or_keyword()?;
            name = format!("{name}.{part}");
        }
        // 泛型实例化 Vec(i32) / IIterable(i32) / Fn1(i32) i32
        let args = if self.at(&TokenKind::LParen) {
            self.advance();
            let mut a = Vec::new();
            if !self.at(&TokenKind::RParen) {
                loop {
                    a.push(self.parse_type()?);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "`)` to close generic args")?;
            // FnN(参数) 返回类型：Fn1(i32) i32——把返回类型并入参数列表
            if name.starts_with("Fn")
                && name.len() > 2
                && name[2..].chars().all(|c| c.is_ascii_digit())
            {
                if let Ok(rt) = self.parse_type() {
                    a.push(rt);
                }
            }
            a
        } else {
            Vec::new()
        };
        Ok(Type::Named(name, args))
    }

    // ---------- 语句 ----------

    fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        let start = self.span();
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "`}`")?;
        let end = self.span();
        Ok(Block {
            stmts,
            span: start.merge(&end),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        // 循环标签：`:label while (...)` / `:label for (...)`（break :label 的对称形式）
        let label = if self.at(&TokenKind::Colon) {
            let l = self.label_if_ident();
            if !matches!(self.peek(), TokenKind::KwWhile | TokenKind::KwFor) {
                return Err(self.error_at("`循环标签`后需跟 `while` 或 `for`"));
            }
            l
        } else {
            None
        };
        let start = self.span();
        match self.peek().clone() {
            TokenKind::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            TokenKind::Semi => {
                self.advance();
                Ok(Stmt::Empty)
            }
            TokenKind::KwVar => {
                self.advance();
                self.parse_var_decl(start)
            }
            TokenKind::KwConst => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq, "`=` in const declaration")?;
                let init = self.parse_expr()?;
                self.expect(&TokenKind::Semi, "`;` after const")?;
                Ok(Stmt::ConstDecl {
                    name,
                    init,
                    span: start,
                })
            }
            TokenKind::KwIf => self.parse_if_stmt(),
            TokenKind::KwWhile => self.parse_while_stmt(label),
            TokenKind::KwFor => self.parse_for_stmt(label),
            TokenKind::KwSwitch => {
                let sw = self.parse_switch()?;
                Ok(Stmt::Switch(sw))
            }
            TokenKind::KwReturn => {
                self.advance();
                let e = if self.at(&TokenKind::Semi) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after return")?;
                Ok(Stmt::Return(e, start.merge(&end)))
            }
            TokenKind::KwBreak => {
                self.advance();
                let label = self.label_if_ident();
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after break")?;
                Ok(Stmt::Break(label, start.merge(&end)))
            }
            TokenKind::KwContinue => {
                self.advance();
                let label = self.label_if_ident();
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after continue")?;
                Ok(Stmt::Continue(label, start.merge(&end)))
            }
            TokenKind::KwDefer => {
                self.advance();
                let e = self.parse_expr()?;
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after defer")?;
                Ok(Stmt::Defer(e, start.merge(&end)))
            }
            TokenKind::KwErrdefer => {
                self.advance();
                let e = self.parse_expr()?;
                let end = self.span();
                self.expect(&TokenKind::Semi, "`;` after errdefer")?;
                Ok(Stmt::Errdefer(e, start.merge(&end)))
            }
            _ => {
                let e = self.parse_assign_or_expr()?;
                // 赋值表达式（已在 parse_assign_or_expr 中构造 Assign）
                self.expect(&TokenKind::Semi, "`;` after expression")?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn label_if_ident(&mut self) -> Option<String> {
        if let TokenKind::Colon = self.peek() {
            // break :label 形式——lexer 已将 : 与标识符分离
            self.advance();
            if let TokenKind::Ident(s) = self.peek() {
                let s = s.clone();
                self.advance();
                return Some(s);
            }
        }
        None
    }

    fn parse_var_decl(&mut self, start: Span) -> Result<Stmt, Diagnostic> {
        let mut_ = if self.at(&TokenKind::KwMut) {
            self.advance();
            true
        } else {
            false
        };
        // 元组解构：var (a, b) = f();
        if self.at(&TokenKind::LParen) {
            self.advance();
            let mut names = Vec::new();
            loop {
                if self.at(&TokenKind::Underscore) {
                    self.advance();
                    names.push("_".to_string());
                } else {
                    names.push(self.expect_ident()?);
                }
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "`)` to close destructure")?;
            self.expect(&TokenKind::Eq, "`=` in destructure")?;
            let init = self.parse_expr()?;
            self.expect(&TokenKind::Semi, "`;` after destructure")?;
            let end = self.span();
            return Ok(Stmt::Expr(Expr::TupleDestructure(
                names,
                Box::new(init),
                start.merge(&end),
            )));
        }
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
        self.expect(&TokenKind::Semi, "`;` after variable declaration")?;
        Ok(Stmt::VarDecl {
            name,
            mut_,
            ty,
            init,
            span: start,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        self.advance(); // if
        self.expect(&TokenKind::LParen, "`(` after if")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after if condition")?;
        let capture = if self.at(&TokenKind::Pipe) {
            let c = self.parse_capture()?;
            Some(c)
        } else {
            None
        };
        let then_b = self.parse_body_or_stmt()?;
        let else_b = if self.at(&TokenKind::KwElse) {
            self.advance();
            if self.at(&TokenKind::KwIf) {
                let inner = self.parse_if_stmt()?;
                Some(Box::new(Stmt::Block(inner_as_block(inner))))
            } else {
                Some(Box::new(Stmt::Block(self.parse_body_or_stmt()?)))
            }
        } else {
            None
        };
        let bspan = then_b.span.clone();
        Ok(Stmt::If(IfStmt {
            cond,
            capture,
            then_b,
            else_b,
            span: bspan,
        }))
    }

    /// 块或单语句（if/while/for 体允许单语句）
    fn parse_body_or_stmt(&mut self) -> Result<Block, Diagnostic> {
        if self.at(&TokenKind::LBrace) {
            self.parse_block()
        } else {
            let start = self.span();
            let s = self.parse_stmt()?;
            let end = self.span();
            Ok(Block {
                stmts: vec![s],
                span: start.merge(&end),
            })
        }
    }

    fn parse_while_stmt(&mut self, label: Option<String>) -> Result<Stmt, Diagnostic> {
        self.advance(); // while
        self.expect(&TokenKind::LParen, "`(` after while")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after while condition")?;
        let step = if self.at(&TokenKind::Colon) {
            self.advance();
            self.expect(&TokenKind::LParen, "`(` in continue step")?;
            let e = self.parse_assign_or_expr()?;
            self.expect(&TokenKind::RParen, "`)` in continue step")?;
            Some(e)
        } else {
            None
        };
        let body = self.parse_body_or_stmt()?;
        let bspan = body.span.clone();
        Ok(Stmt::While(WhileStmt {
            label,
            cond,
            step,
            body,
            span: bspan,
        }))
    }

    fn parse_for_stmt(&mut self, label: Option<String>) -> Result<Stmt, Diagnostic> {
        self.advance(); // for
        self.expect(&TokenKind::LParen, "`(` after for")?;
        let iter = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after for iterable")?;
        let (mode, name) = self.parse_capture()?;
        let body = self.parse_body_or_stmt()?;
        let bspan = body.span.clone();
        Ok(Stmt::For(ForStmt {
            label,
            iter,
            capture: mode,
            capture_name: name,
            body,
            span: bspan,
        }))
    }

    /// 解析表达式，若遇到复合赋值运算符则构造 Assign（continue step / 语句场景）
    fn parse_assign_or_expr(&mut self) -> Result<Expr, Diagnostic> {
        let e = self.parse_expr()?;
        let aop = match self.peek() {
            TokenKind::Eq => Some(AssignOp::Set),
            TokenKind::PlusEq => Some(AssignOp::Add),
            TokenKind::MinusEq => Some(AssignOp::Sub),
            TokenKind::StarEq => Some(AssignOp::Mul),
            TokenKind::SlashEq => Some(AssignOp::Div),
            TokenKind::PipeEq => Some(AssignOp::BitOr),
            TokenKind::AmpEq => Some(AssignOp::BitAnd),
            TokenKind::CaretEq => Some(AssignOp::BitXor),
            _ => None,
        };
        if let Some(op) = aop {
            self.advance();
            let rhs = self.parse_expr()?;
            let span = e.span().merge(&rhs.span());
            let target = match &e {
                Expr::Ident(_, _)
                | Expr::Index { .. }
                | Expr::Field { .. }
                | Expr::Dot { .. }
                | Expr::Deref(_, _) => e.clone(),
                _ => return Err(Diagnostic::error(e.span(), "invalid assignment target")),
            };
            return Ok(Expr::Assign {
                target: Box::new(target),
                op,
                value: Box::new(rhs),
                span,
            });
        }
        Ok(e)
    }

    /// switch 臂内的语句：return/break/continue 后不强制分号（臂以 `,` 分隔）
    fn parse_switch_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.span();
        match self.peek() {
            TokenKind::KwReturn => {
                self.advance();
                let e = if matches!(
                    self.peek(),
                    TokenKind::Comma | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                let end = self.span();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Return(e, start.merge(&end)))
            }
            TokenKind::KwBreak => {
                self.advance();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Break(None, start))
            }
            TokenKind::KwContinue => {
                self.advance();
                if self.at(&TokenKind::Semi) {
                    self.advance();
                }
                Ok(Stmt::Continue(None, start))
            }
            _ => self.parse_stmt(),
        }
    }

    fn parse_capture(&mut self) -> Result<(CaptureMode, String), Diagnostic> {
        self.expect(&TokenKind::Pipe, "`|` to open capture")?;
        let mode = if self.at(&TokenKind::KwMut) {
            self.advance();
            CaptureMode::Mut
        } else if self.at(&TokenKind::KwMove) {
            self.advance();
            CaptureMode::Move
        } else {
            CaptureMode::Read
        };
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Pipe, "`|` to close capture")?;
        Ok((mode, name))
    }

    fn parse_switch(&mut self) -> Result<SwitchStmt, Diagnostic> {
        self.advance(); // switch
        self.expect(&TokenKind::LParen, "`(` after switch")?;
        let subject = self.parse_expr()?;
        self.expect(&TokenKind::RParen, "`)` after switch subject")?;
        self.expect(&TokenKind::LBrace, "`{` to open switch")?;
        let mut arms = Vec::new();
        let mut has_else = false;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let astart = self.span();
            let mut patterns = Vec::new();
            loop {
                patterns.push(self.parse_switch_pattern()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            if matches!(patterns.last(), Some(SwitchPattern::Else)) {
                has_else = true;
            }
            self.expect(&TokenKind::FatArrow, "`=>` in switch arm")?;
            let capture = if self.at(&TokenKind::Pipe) {
                let (m, n) = self.parse_capture()?;
                Some((m, n))
            } else {
                None
            };
            // 臂体可以是块、表达式或语句（`=> return error.NotFound`）
            let body = if self.at(&TokenKind::LBrace) {
                self.parse_block()?
            } else if matches!(
                self.peek(),
                TokenKind::KwReturn
                    | TokenKind::KwBreak
                    | TokenKind::KwContinue
                    | TokenKind::KwVar
                    | TokenKind::KwConst
            ) {
                let start = self.span();
                let s = self.parse_switch_stmt()?;
                let end = self.span();
                Block {
                    stmts: vec![s],
                    span: start.merge(&end),
                }
            } else {
                let e = self.parse_expr()?;
                let span = e.span();
                Block {
                    stmts: vec![Stmt::Expr(e)],
                    span,
                }
            };
            let end = self.span();
            arms.push(SwitchArm {
                patterns,
                capture,
                body,
                span: astart.merge(&end),
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close switch")?;
        let end = self.span();
        Ok(SwitchStmt {
            subject,
            arms,
            has_else,
            span: end,
        })
    }

    fn parse_switch_pattern(&mut self) -> Result<SwitchPattern, Diagnostic> {
        let start = self.span();
        // error.NotFound
        if self.is_ident("error") && self.peek_n(1) == &TokenKind::Dot {
            self.advance();
            self.advance();
            let name = self.expect_ident()?;
            return Ok(SwitchPattern::Error(name));
        }
        // 枚举限定模式 Direction.north / Type.variant（变体名可为关键字，如 JsonValue.null）
        if let TokenKind::Ident(_) = self.peek() {
            if self.peek_n(1) == &TokenKind::Dot {
                let _ty = self.expect_ident()?;
                self.advance(); // .
                let variant = self.expect_name_or_keyword()?;
                return Ok(SwitchPattern::Ident(variant));
            }
        }
        match self.peek().clone() {
            TokenKind::KwElse => {
                self.advance();
                Ok(SwitchPattern::Else)
            }
            TokenKind::KwNull => {
                self.advance();
                Ok(SwitchPattern::Ident("null".into()))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(SwitchPattern::Ident("true".into()))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(SwitchPattern::Ident("false".into()))
            }
            TokenKind::Int(s) => {
                self.advance();
                Ok(SwitchPattern::Int(s))
            }
            TokenKind::Float(s) => {
                self.advance();
                Ok(SwitchPattern::Float(s))
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(SwitchPattern::Str(s))
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(SwitchPattern::Char(c))
            }
            TokenKind::Ident(s) => {
                self.advance();
                Ok(SwitchPattern::Ident(s))
            }
            other => Err(Diagnostic::error(
                start,
                format!("invalid switch pattern: {}", other.describe()),
            )),
        }
    }

    // ---------- 表达式 ----------

    pub fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_and()?;
        while self.at(&TokenKind::KwOr) || self.at(&TokenKind::PipePipe) {
            let op = self.advance();
            let r = self.parse_and()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::Or, Box::new(l), Box::new(r), span);
            let _ = op;
        }
        Ok(l)
    }

    fn parse_and(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_range()?;
        while self.at(&TokenKind::KwAnd) {
            self.advance();
            let r = self.parse_range()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::And, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    /// 区间糖 0..10（Q29：复用 .. 记号）；无上界 `1..`（切片到末尾）
    fn parse_range(&mut self) -> Result<Expr, Diagnostic> {
        let l = self.parse_comparison()?;
        if self.at(&TokenKind::DotDot) {
            self.advance();
            // 无上界：后随 ]/)/,/; 等边界符
            let open_end = matches!(
                self.peek(),
                TokenKind::RBracket | TokenKind::RParen | TokenKind::Comma | TokenKind::Semi
            );
            if open_end {
                let span = l.span().clone();
                let end_marker = Expr::IntLit {
                    text: "__end__".into(),
                    span: span.clone(),
                };
                return Ok(Expr::Binary(
                    BinOp::Range,
                    Box::new(l),
                    Box::new(end_marker),
                    span,
                ));
            }
            let r = self.parse_comparison()?;
            let span = l.span().merge(&r.span());
            return Ok(Expr::Binary(BinOp::Range, Box::new(l), Box::new(r), span));
        }
        Ok(l)
    }

    fn parse_comparison(&mut self) -> Result<Expr, Diagnostic> {
        let l = self.parse_bitor()?;
        let op = match self.peek() {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::Ne => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Le => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let r = self.parse_bitor()?;
            let span = l.span().merge(&r.span());
            return Ok(Expr::Binary(op, Box::new(l), Box::new(r), span));
        }
        Ok(l)
    }

    fn parse_bitor(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_bitxor()?;
        while self.at(&TokenKind::Pipe) {
            self.advance();
            let r = self.parse_bitxor()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitOr, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_bitand()?;
        while self.at(&TokenKind::Caret) {
            self.advance();
            let r = self.parse_bitand()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitXor, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_bitand(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_shift()?;
        while self.at(&TokenKind::Amp) {
            self.advance();
            let r = self.parse_shift()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(BinOp::BitAnd, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_shift(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_addsub()?;
        loop {
            let op = match self.peek() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let r = self.parse_addsub()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_addsub(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_muldiv()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let r = self.parse_muldiv()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_muldiv(&mut self) -> Result<Expr, Diagnostic> {
        let mut l = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::PercentPercent => BinOp::EucMod,
                _ => break,
            };
            self.advance();
            let r = self.parse_unary()?;
            let span = l.span().merge(&r.span());
            l = Expr::Binary(op, Box::new(l), Box::new(r), span);
        }
        Ok(l)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.span();
        match self.peek() {
            TokenKind::Minus => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(e), start))
            }
            TokenKind::Bang => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Not, Box::new(e), start))
            }
            TokenKind::Tilde => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::BitNot, Box::new(e), start))
            }
            TokenKind::Amp => {
                self.advance();
                let mut_ = if self.at(&TokenKind::KwMut) {
                    self.advance();
                    true
                } else {
                    false
                };
                let e = self.parse_unary()?;
                Ok(Expr::AddrOf(Box::new(e), mut_, start))
            }
            TokenKind::KwTry => {
                self.advance();
                let e = self.parse_unary()?;
                Ok(Expr::Try(Box::new(e), start))
            }
            TokenKind::KwSpawn => {
                // E2.2：`spawn(f, args...) o Thread(T)`——以普通调用形态解析
                // （callee = Ident("spawn")），语义层按内建处理（is_builtin_fn "spawn"）。
                // 第三块 E2 的 async/await/spawn 并发语法不在此列，保持 out-of-scope。
                self.advance();
                let args = self.parse_call_args()?;
                let end = self.span();
                Ok(Expr::Call {
                    callee: Box::new(Expr::Ident("spawn".to_string(), start.clone())),
                    args,
                    span: start.merge(&end),
                })
            }
            TokenKind::KwMove => {
                // move |v| ... = move 捕获闭包；否则 move x（所有权转移标记，M2.4——
                // 保留供语义检查器验证唯一约束 = 拥有所有权；原绑定仍可访问，悬垂/冲突由用户负责）
                if self.peek_n(1) == &TokenKind::Pipe
                    || (self.peek_n(1) == &TokenKind::KwMut && self.peek_n(2) == &TokenKind::Pipe)
                {
                    return self.parse_closure(start);
                }
                self.advance();
                let inner = self.parse_unary()?;
                let end = self.span();
                Ok(Expr::Move(Box::new(inner), start.merge(&end)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut e = self.parse_primary()?;
        loop {
            let start = self.span();
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    // `.?` 链式解包（m.get("apple").?）
                    if self.at(&TokenKind::Question) {
                        self.advance();
                        e = Expr::Unwrap(Box::new(e), start);
                        continue;
                    }
                    let field = self.expect_name_or_keyword()?;
                    // 调用：p.dist(q)
                    if self.at(&TokenKind::LParen) {
                        let args = self.parse_call_args()?;
                        let end = self.span();
                        e = Expr::Call {
                            callee: Box::new(Expr::Field {
                                base: Box::new(e),
                                field,
                                span: start.clone(),
                            }),
                            args,
                            span: start.merge(&end),
                        };
                    } else {
                        e = Expr::Field {
                            base: Box::new(e),
                            field,
                            span: start,
                        };
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    let mut indices = vec![self.parse_expr()?];
                    while self.at(&TokenKind::Comma) {
                        self.advance();
                        indices.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RBracket, "`]` after index")?;
                    let end = self.span();
                    e = Expr::Index {
                        base: Box::new(e),
                        indices,
                        span: start.merge(&end),
                    };
                }
                TokenKind::DotStar => {
                    self.advance();
                    e = Expr::Deref(Box::new(e), start);
                }
                TokenKind::Question => {
                    self.advance();
                    e = Expr::Unwrap(Box::new(e), start);
                }
                TokenKind::LParen => {
                    let args = self.parse_call_args()?;
                    let end = self.span();
                    e = Expr::Call {
                        callee: Box::new(e),
                        args,
                        span: start.merge(&end),
                    };
                    // 泛型类型实例化后字面量：Pair(i32){ first = 1, ... }。
                    // call 实参按类型实参收集（E1.2 组 D comptime 类型应用——不再丢弃）。
                    if self.at(&TokenKind::LBrace) {
                        match &e {
                            Expr::Call { callee, args, .. }
                                if matches!(callee.as_ref(), Expr::Ident(_, _)) =>
                            {
                                if let Expr::Ident(tyname, _) = callee.as_ref() {
                                    let ty_args: Vec<Type> = args
                                        .iter()
                                        .filter_map(|a| self.expr_to_type(a))
                                        .collect();
                                    let fields = self.parse_named_lit_fields()?;
                                    e = Expr::NamedLit {
                                        ty: tyname.clone(),
                                        ty_args,
                                        fields,
                                        span: start,
                                    };
                                }
                            }
                            _ => {
                                let _ = self.parse_named_lit_fields()?;
                            }
                        }
                    }
                }
                TokenKind::KwOrelse => {
                    self.advance();
                    // orelse return/continue/break（tag1：控制流兜底 → Block 表达式；
                    // return 不消费分号——由外层声明消费）
                    if self.at(&TokenKind::KwReturn) {
                        self.advance();
                        let rv = if self.at(&TokenKind::Semi) {
                            None
                        } else {
                            Some(self.parse_expr()?)
                        };
                        let sp = start.clone();
                        let block = Block {
                            stmts: vec![Stmt::Return(rv, sp.clone())],
                            span: sp.clone(),
                        };
                        let end = self.span();
                        e = Expr::Orelse(
                            Box::new(e),
                            Box::new(Expr::Block(block, sp)),
                            start.merge(&end),
                        );
                    } else if self.at(&TokenKind::KwContinue) || self.at(&TokenKind::KwBreak) {
                        let kw = self.advance();
                        let stmt = match kw.kind {
                            TokenKind::KwContinue => Stmt::Continue(None, kw.span.clone()),
                            _ => Stmt::Break(None, kw.span.clone()),
                        };
                        // 不消费分号——由外层声明消费
                        let sp = start.clone();
                        let block = Block {
                            stmts: vec![stmt],
                            span: sp.clone(),
                        };
                        let end = self.span();
                        e = Expr::Orelse(
                            Box::new(e),
                            Box::new(Expr::Block(block, sp)),
                            start.merge(&end),
                        );
                    } else {
                        let r = self.parse_expr()?;
                        let end = self.span();
                        e = Expr::Orelse(Box::new(e), Box::new(r), start.merge(&end));
                    }
                }
                TokenKind::KwCatch => {
                    self.advance();
                    let kind = if self.at(&TokenKind::Pipe) {
                        let (_, name) = self.parse_capture()?;
                        // 体：块或表达式（`catch |err| switch (err) { ... }`）
                        let body = if self.at(&TokenKind::LBrace) {
                            self.parse_block()?
                        } else {
                            let e = self.parse_expr()?;
                            let sp = e.span();
                            Block {
                                stmts: vec![Stmt::Expr(e)],
                                span: sp,
                            }
                        };
                        CatchKind::Bind { name, body }
                    } else if self.at(&TokenKind::KwReturn) {
                        // catch return error.X（不消费分号——外层 return 消费）
                        self.advance();
                        let rv = if self.at(&TokenKind::Semi) {
                            None
                        } else {
                            Some(self.parse_expr()?)
                        };
                        let body = Block {
                            stmts: vec![Stmt::Return(rv, start.clone())],
                            span: start.clone(),
                        };
                        CatchKind::Bind {
                            name: "__catch_flow__".into(),
                            body,
                        }
                    } else if self.at(&TokenKind::KwBreak) || self.at(&TokenKind::KwContinue) {
                        let stmt = self.parse_stmt()?;
                        let body = Block {
                            stmts: vec![stmt],
                            span: start.clone(),
                        };
                        CatchKind::Bind {
                            name: "__catch_flow__".into(),
                            body,
                        }
                    } else {
                        let default = self.parse_expr()?;
                        CatchKind::Default(Box::new(default))
                    };
                    let end = self.span();
                    e = Expr::Catch(Box::new(e), Box::new(kind), start.merge(&end));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    // 尾随逗号
                    if self.at(&TokenKind::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.span();
        // 闭包：|v| expr / |v, w| expr / mut |v| { ... }
        if self.at(&TokenKind::Pipe)
            || (self.at(&TokenKind::KwMut) && self.peek_n(1) == &TokenKind::Pipe)
        {
            return self.parse_closure(start);
        }
        // 推断枚举值字面量（L1）：.shallow ≡ CopyMode.shallow——参数/字段类型已知时
        if self.at(&TokenKind::Dot) {
            self.advance();
            let variant = self.expect_name_or_keyword()?;
            let end = self.span();
            return Ok(Expr::Dot {
                base: Box::new(Expr::VoidLit(start.clone())),
                field: variant,
                span: start.merge(&end),
            });
        }
        // @ 内建表达式：@intFromEnum(k)
        if let TokenKind::AtBuiltin(name) = self.peek().clone() {
            self.advance();
            let args = self.parse_call_args()?;
            let end = self.span();
            return Ok(Expr::Call {
                callee: Box::new(Expr::Ident(format!("@{name}"), start.clone())),
                args,
                span: start.merge(&end),
            });
        }
        match self.peek().clone() {
            // struct { ... } 类型字面量（H1：struct/class 合并；E1.2 组 D type-as-value）。
            // 字段为 `name: Type`（类型标注——保留，构成 `Expr::StructType` 类型值）或
            // `name = expr`（值——tag1 仅解析不执行，NamedLit 占位）。
            TokenKind::KwClass => {
                self.advance();
                self.expect(&TokenKind::LBrace, "`{` after struct literal")?;
                let mut type_fields: Vec<(String, Type)> = Vec::new();
                let mut value_fields: Vec<(String, Expr)> = Vec::new();
                let mut all_typed = true;
                if !self.at(&TokenKind::RBrace) {
                    loop {
                        let name = self.expect_ident()?;
                        if self.at(&TokenKind::Colon) {
                            self.advance();
                            let ty = self.parse_type()?;
                            type_fields.push((name, ty));
                        } else if self.at(&TokenKind::Eq) {
                            self.advance();
                            let v = self.parse_expr()?;
                            value_fields.push((name, v));
                            all_typed = false;
                        } else {
                            return Err(self.error_at(format!(
                                "expected `:` or `=` in struct literal field `{name}`"
                            )));
                        }
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            if self.at(&TokenKind::RBrace) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBrace, "`}` after struct literal")?;
                let end = self.span();
                if all_typed {
                    Ok(Expr::StructType {
                        fields: type_fields,
                        span: start.merge(&end),
                    })
                } else {
                    Ok(Expr::NamedLit {
                        ty: "struct".into(),
                        ty_args: vec![],
                        fields: value_fields,
                        span: start.merge(&end),
                    })
                }
            }
            TokenKind::Int(s) => {
                self.advance();
                Ok(Expr::IntLit {
                    text: s,
                    span: start,
                })
            }
            TokenKind::Float(s) => {
                self.advance();
                Ok(Expr::FloatLit {
                    text: s,
                    span: start,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr::StrLit {
                    value: s,
                    raw: false,
                    span: start,
                })
            }
            TokenKind::RawStr(s) => {
                self.advance();
                Ok(Expr::StrLit {
                    value: s,
                    raw: true,
                    span: start,
                })
            }
            TokenKind::Char(c) => {
                self.advance();
                Ok(Expr::CharLit(c, start))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(Expr::BoolLit(true, start))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(Expr::BoolLit(false, start))
            }
            TokenKind::KwNull => {
                self.advance();
                Ok(Expr::NullLit(start))
            }
            TokenKind::KwVoid => {
                self.advance();
                Ok(Expr::VoidLit(start))
            }
            TokenKind::KwIf => {
                self.advance();
                self.expect(&TokenKind::LParen, "`(` after if")?;
                let cond = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                // 捕获形式 if (opt) |v| ...
                let capture = if self.at(&TokenKind::Pipe) {
                    let c = self.parse_capture()?;
                    Some(c)
                } else {
                    None
                };
                let then_e = self.parse_expr()?;
                self.expect(&TokenKind::KwElse, "`else` in if expression")?;
                let else_e = self.parse_expr()?;
                let end = self.span();
                Ok(Expr::IfExpr {
                    cond: Box::new(cond),
                    capture,
                    then_e: Box::new(then_e),
                    else_e: Box::new(else_e),
                    span: start.merge(&end),
                })
            }
            TokenKind::KwSwitch => {
                let sw = self.parse_switch()?;
                Ok(Expr::SwitchExpr {
                    subject: Box::new(sw.subject),
                    arms: sw.arms,
                    span: sw.span,
                })
            }
            TokenKind::LBrace => {
                let b = self.parse_block()?;
                Ok(Expr::Block(b, start))
            }
            TokenKind::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.at(&TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            // 尾逗号：`[a, b, ]`
                            if self.at(&TokenKind::RBracket) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "`]` after array literal")?;
                let end = self.span();
                Ok(Expr::ArrayLit(items, start.merge(&end)))
            }
            TokenKind::LParen => {
                self.advance();
                if self.at(&TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::TupleLit(vec![], start));
                }
                let first = self.parse_expr()?;
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    let mut items = vec![first];
                    while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                        items.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)` to close tuple")?;
                    let end = self.span();
                    Ok(Expr::TupleLit(items, start.merge(&end)))
                } else {
                    self.expect(&TokenKind::RParen, "`)`")?;
                    Ok(first) // 括号分组：直接返回内层表达式（短路语义依赖）
                }
            }
            TokenKind::KwTry => {
                self.advance();
                let e = self.parse_expr()?;
                Ok(Expr::Try(Box::new(e), start))
            }
            TokenKind::Ident(name) => {
                self.advance();
                // struct 关键字（已合并入 class，2026-08-14）：匿名类型占位跳过
                if name == "struct" && self.at(&TokenKind::LBrace) {
                    self.skip_anon_struct()?;
                    let end = self.span();
                    return Ok(Expr::VoidLit(start.merge(&end)));
                }
                // error.NotFound 错误字面量
                if name == "error" && self.at(&TokenKind::Dot) {
                    self.advance();
                    let ename = self.expect_ident()?;
                    return Ok(Expr::ErrorLit(ename, start));
                }
                // 集合类型实例化 Vec(i32)/Map(&[u8], i32)/Table(i32)：泛型参数为类型（跳过），
                // 返回 Ident 以便 postfix `.init` 继续（tag1：类型实例化 = 空容器）
                if matches!(name.as_str(), "Vec" | "Map" | "Deque" | "Table")
                    && self.at(&TokenKind::LParen)
                {
                    self.advance();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            let _ = self.parse_type()?;
                            if self.at(&TokenKind::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)` to close collection type")?;
                    return Ok(Expr::Ident(name, start));
                }
                // Type.name（枚举常量）——解释器统一处理
                // 字面量构造 Type{...} 或调用 Type(...)
                if self.at(&TokenKind::LBrace) {
                    let fields = self.parse_named_lit_fields()?;
                    let end = self.span();
                    return Ok(Expr::NamedLit {
                        ty: name,
                        ty_args: vec![],
                        fields,
                        span: start.merge(&end),
                    });
                }
                if self.at(&TokenKind::Dot) {
                    self.advance();
                    // `x.?` 链式解包（对齐 parse_postfix 形态）：裸标识符解包
                    if self.at(&TokenKind::Question) {
                        self.advance();
                        let end = self.span();
                        return Ok(Expr::Unwrap(
                            Box::new(Expr::Ident(name, start.clone())),
                            start.merge(&end),
                        ));
                    }
                    let field = self.expect_name_or_keyword()?;
                    let end = self.span();
                    // M1.4 限定名类型字面量：Orders.Line{ ... }
                    if self.at(&TokenKind::LBrace) {
                        let fields = self.parse_named_lit_fields()?;
                        let end = self.span();
                        return Ok(Expr::NamedLit {
                            ty: format!("{name}.{field}"),
                            ty_args: vec![],
                            fields,
                            span: start.merge(&end),
                        });
                    }
                    if self.at(&TokenKind::LParen) {
                        let args = self.parse_call_args()?;
                        let span = start.merge(&end);
                        return Ok(Expr::Call {
                            callee: Box::new(Expr::Dot {
                                base: Box::new(Expr::Ident(name, start.clone())),
                                field,
                                span: start.clone(),
                            }),
                            args,
                            span,
                        });
                    }
                    return Ok(Expr::Dot {
                        base: Box::new(Expr::Ident(name, start.clone())),
                        field,
                        span: start.merge(&end),
                    });
                }
                Ok(Expr::Ident(name, start))
            }
            other => Err(Diagnostic::error(
                start,
                format!("expected expression, found {}", other.describe()),
            )),
        }
    }

    /// 闭包：|v| expr / |v, w| { ... } / mut |v| ... / move |v| ...（Q13：FnN 调用接口）
    fn parse_closure(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        // 捕获模式前缀：mut（可写）/ move（转移捕获）——可叠放，粒度 = 整个闭包
        let mut is_mut = false;
        let mut is_move = false;
        loop {
            if self.at(&TokenKind::KwMut) {
                self.advance();
                is_mut = true;
            } else if self.at(&TokenKind::KwMove) {
                self.advance();
                is_move = true;
            } else {
                break;
            }
        }
        self.expect(&TokenKind::Pipe, "`|` to open closure params")?;
        let mut params = Vec::new();
        if !self.at(&TokenKind::Pipe) {
            loop {
                params.push(self.expect_ident()?);
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::Pipe, "`|` to close closure params")?;
        let body = if self.at(&TokenKind::LBrace) {
            self.parse_block()?
        } else {
            let e = self.parse_expr()?;
            let sp = e.span();
            Block {
                stmts: vec![Stmt::Expr(e)],
                span: sp,
            }
        };
        let end = self.span();
        Ok(Expr::Closure {
            params,
            body,
            is_mut,
            is_move,
            span: start.merge(&end),
        })
    }

    fn parse_named_lit_fields(&mut self) -> Result<Vec<(String, Expr)>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let name = self.expect_ident()?;
                self.expect(&TokenKind::Eq, "`=` in literal field")?;
                let value = self.parse_expr()?;
                fields.push((name, value));
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    if self.at(&TokenKind::RBrace) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}` after literal")?;
        Ok(fields)
    }

    /// 表达式 → 类型实参（E1.2 组 D）：`Pair(i32){...}` 的 call 实参转为泛型实参。
    /// 支持裸标识符 `i32`、限定名 `Math.Vec`、嵌套泛型 `Vec(i32)`；非类型形态返回 None
    /// （调用方按「无类型实参」处理——tag1 泛型实参必须可解析为类型）。
    fn expr_to_type(&self, e: &Expr) -> Option<Type> {
        match e {
            Expr::Ident(n, _) => Some(Type::Named(n.clone(), vec![])),
            Expr::Dot { base, field, .. } => match self.expr_to_type(base) {
                Some(Type::Named(bn, bargs)) if bargs.is_empty() => {
                    Some(Type::Named(format!("{bn}.{field}"), vec![]))
                }
                _ => None,
            },
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                Expr::Ident(n, _) => {
                    let targs: Option<Vec<Type>> =
                        args.iter().map(|a| self.expr_to_type(a)).collect();
                    targs.map(|t| Type::Named(n.clone(), t))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn expect_ident(&mut self) -> Result<String, Diagnostic> {
        match self.peek() {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            other => Err(Diagnostic::error(
                self.span(),
                format!("expected identifier, found {}", other.describe()),
            )),
        }
    }

    /// 函数/方法名：允许关键字（如方法名 where / 变体名 null）
    fn expect_name_or_keyword(&mut self) -> Result<String, Diagnostic> {
        match self.peek() {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            // 方法名可为关键字（tag1：where 等）
            TokenKind::KwWhere => {
                self.advance();
                Ok("where".to_string())
            }
            TokenKind::KwNull => {
                self.advance();
                Ok("null".to_string())
            }
            // M7.2：`Kind.script`（build.zon kind 值）——script 为关键字，作名称放行
            TokenKind::KwScript => {
                self.advance();
                Ok("script".to_string())
            }
            // E1（ADR-0013）：`types.type`（当前类型名元数据）——type 为关键字，点号后作名称放行
            TokenKind::KwType => {
                self.advance();
                Ok("type".to_string())
            }
            other => Err(Diagnostic::error(
                self.span(),
                format!("expected identifier, found {}", other.describe()),
            )),
        }
    }

    fn is_ident(&self, name: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(s) if s == name)
    }
}

fn inner_as_block(s: Stmt) -> Block {
    match s {
        Stmt::Block(b) => b,
        other => Block {
            stmts: vec![other],
            span: Span::new(0, 0, 0, 0),
        },
    }
}

// 兼容测试用
impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit { span, .. }
            | Expr::FloatLit { span, .. }
            | Expr::StrLit { span, .. }
            | Expr::CharLit(_, span)
            | Expr::BoolLit(_, span)
            | Expr::NullLit(span)
            | Expr::VoidLit(span)
            | Expr::Ident(_, span)
            | Expr::ArrayLit(_, span)
            | Expr::TupleLit(_, span)
            | Expr::NamedLit { span, .. }
            | Expr::StructType { span, .. }
            | Expr::Dot { span, .. }
            | Expr::Field { span, .. }
            | Expr::Index { span, .. }
            | Expr::Deref(_, span)
            | Expr::AddrOf(_, _, span)
            | Expr::Unary(_, _, span)
            | Expr::Binary(_, _, _, span)
            | Expr::Orelse(_, _, span)
            | Expr::Unwrap(_, span)
            | Expr::Try(_, span)
            | Expr::Catch(_, _, span)
            | Expr::Call { span, .. }
            | Expr::IfExpr { span, .. }
            | Expr::SwitchExpr { span, .. }
            | Expr::Block(_, span)
            | Expr::Assign { span, .. }
            | Expr::ErrorLit(_, span)
            | Expr::FnRef(_, span)
            | Expr::TupleDestructure(_, _, span)
            | Expr::Move(_, span)
            | Expr::Closure { span, .. } => span.clone(),
        }
    }
}
