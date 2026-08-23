//! Parser 类型声明解析：class / enum / union / interface / 路径。

use super::*;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::token::TokenKind;

impl Parser {
    pub(crate) fn parse_class(
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
                let (mname, mtype_params, mparams, mret, mwhere, mbody, mspan) =
                    self.parse_fn_rest(mstart)?;
                methods.push(Method {
                    name: mname,
                    type_params: mtype_params,
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
                    traits: vec![],
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

    pub(crate) fn parse_struct(
        &mut self,
        start: Span,
        traits: Vec<Trait>,
        is_pub: bool,
    ) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "`{` to open struct body")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            // 字段级 [Align(n)] 特性
            let mut field_traits = Vec::new();
            while self.at(&TokenKind::LBracket) {
                if let Some(t) = self.parse_trait()? {
                    field_traits.push(t);
                }
            }
            // 字段可见性
            let member_pub = if self.at(&TokenKind::KwPub) {
                self.advance();
                true
            } else {
                false
            };
            let fstart = self.span();
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "`:` after field name")?;
            let fty = self.parse_type()?;
            // 字段默认值
            let default = if self.at(&TokenKind::Eq) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            fields.push(FieldDecl {
                name: fname,
                ty: fty,
                pub_: member_pub,
                span: fstart,
                traits: field_traits,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else if !self.at(&TokenKind::RBrace) {
                return Err(self.error_at("expected `,` or `}` after struct field"));
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close struct body")?;
        let end = self.span();
        Ok(Decl::Struct {
            name,
            traits,
            fields,
            pub_: is_pub,
            span: start.merge(&end),
        })
    }

    pub(crate) fn parse_enum(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
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

    /// K1（ADR-0014）：无标签 union 声明解析——仅字段（无方法/无接口），
    /// 字段语法同 class（`name: Type,`）。字段内存重叠语义在语义/运行时层落实。
    pub(crate) fn parse_union(&mut self, start: Span, is_pub: bool) -> Result<Decl, Diagnostic> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LBrace, "`{` to open union body")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            // 字段：name: Type,（union 无方法；`pub` 成员标注同 class——属性默认私有）
            let member_pub = if self.at(&TokenKind::KwPub) {
                self.advance();
                true
            } else {
                false
            };
            if self.at(&TokenKind::KwMut) {
                self.advance();
            }
            let fstart = self.span();
            let fname = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "`:` after union field name")?;
            let fty = self.parse_type()?;
            fields.push(FieldDecl {
                name: fname,
                ty: fty,
                pub_: member_pub,
                traits: vec![],
                span: fstart,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else if !self.at(&TokenKind::RBrace) {
                return Err(self.error_at("expected `,` or `}` after union field"));
            }
        }
        self.expect(&TokenKind::RBrace, "`}` to close union body")?;
        let end = self.span();
        Ok(Decl::Union {
            name,
            fields,
            pub_: is_pub,
            span: start.merge(&end),
        })
    }

    /// 接口方法（tag1 已支持）；解析返回类型 `!void` 与 `where` 子句
    pub(crate) fn parse_interface_method(&mut self) -> Result<Method, Diagnostic> {
        self.advance(); // fn
        let mstart = self.span();
        let name = self.expect_ident()?;
        // 泛型参数表：`fn save<T>(...)`（接口方法）
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
            type_params,
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

    pub(crate) fn parse_interface(
        &mut self,
        start: Span,
        is_pub: bool,
    ) -> Result<Decl, Diagnostic> {
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
    pub(crate) fn skip_anon_struct(&mut self) -> Result<(), Diagnostic> {
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

    pub(crate) fn parse_path(&mut self) -> Result<Vec<String>, Diagnostic> {
        let mut path = vec![self.expect_ident()?];
        while self.at(&TokenKind::Dot) {
            self.advance();
            path.push(self.expect_ident()?);
        }
        Ok(path)
    }

    /// import 路径：同 `parse_path`，但符号选择 `.{`（`.` 后跟 `{`）止步——
    /// `import H.std.{io as my};` 的 `H.std` 到 `.{` 为止。
    pub(crate) fn parse_import_path(&mut self) -> Result<Vec<String>, Diagnostic> {
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
}
