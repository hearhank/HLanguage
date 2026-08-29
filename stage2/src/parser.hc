// stage2/src/parser.hc — 阶段 2：语法分析 + AST 模型 + AST 输出
// S3：从 stage1/interp.hc 内嵌副本提取（含 K5-pre 修复：switch 多模式臂、
// CharLit 十进制 value prop、import 选择集 syms prop）。
// 同命名空间扁平共享（ADR-0031）：Token/Lexer 来自 src/lexer.hc，直接互见。

fn build_rev_kw_map() Map<&[u8], &[u8]> {
    var m = Map<&[u8], &[u8]>.init(alloc);
    m.put("KwAnd", "and");
    m.put("KwAnytype", "anytype");
    m.put("KwAsync", "async");
    m.put("KwAwait", "await");
    m.put("KwBreak", "break");
    m.put("KwCatch", "catch");
    m.put("KwClass", "class");
    m.put("KwComptime", "comptime");
    m.put("KwConst", "const");
    m.put("KwContinue", "continue");
    m.put("KwDefer", "defer");
    m.put("KwElse", "else");
    m.put("KwEnum", "enum");
    m.put("KwErrdefer", "errdefer");
    m.put("KwExport", "export");
    m.put("KwExtern", "extern");
    m.put("KwFalse", "false");
    m.put("KwFn", "fn");
    m.put("KwFor", "for");
    m.put("KwGlobal", "global");
    m.put("KwIf", "if");
    m.put("KwImport", "import");
    m.put("KwInterface", "interface");
    m.put("KwMove", "move");
    m.put("KwMut", "mut");
    m.put("KwNamespace", "namespace");
    m.put("KwNull", "null");
    m.put("KwOr", "or");
    m.put("KwOrelse", "orelse");
    m.put("KwOwned", "owned");
    m.put("KwPub", "pub");
    m.put("KwReturn", "return");
    m.put("KwScript", "script");
    m.put("KwSpawn", "spawn");
    m.put("KwSwitch", "switch");
    m.put("KwTree", "tree");
    m.put("KwTrue", "true");
    m.put("KwTry", "try");
    m.put("KwType", "type");
    m.put("KwUnion", "union");
    m.put("KwImport", "import");
    m.put("KwVar", "var");
    m.put("KwVoid", "void");
    m.put("KwWhere", "where");
    m.put("KwWhile", "while");
    return m;
}
// ============================================================
// AST 类型定义
// ============================================================

// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<Prop> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

// 简单的 AST：用 Vec<u8> 存储节点类型名，属性用 Vec<u8> 字符串键值对
// 输出格式：NodeType|key=val|key=val\n  children (indented)

class AstNode {
    kind: &[u8],
    // props: flat Vec<u8> with key=value pairs separated by null
    props: Vec<u8>,
    // children
    children: Vec<AstNode>,
}

fn make_node(kind: &[u8]) AstNode {
    var n = AstNode{
        kind = kind,
        props = Vec<u8>.init(alloc),
        children = Vec<AstNode>.init(alloc),
    };
    return n;
}

fn node_add_prop(node: *mut AstNode, key: &[u8], val: &[u8]) void {
    // encode: |key=value
    node.props.append('|');
    var mut i: usize = 0;
    while (i < key.len) {
        node.props.append(key[i]);
        i += 1;
    }
    node.props.append('=');
    i = 0;
    while (i < val.len) {
        node.props.append(val[i]);
        i += 1;
    }
}

fn node_add_child(node: *mut AstNode, child: AstNode) void {
    node.children.append(child);
}

fn quoted_add_prop(node: *mut AstNode, key: &[u8], val: &[u8]) void {
    node.props.append('|');
    var mut i: usize = 0;
    while (i < key.len) {
        node.props.append(key[i]);
        i += 1;
    }
    node.props.append('=');
    node.props.append('"');
    i = 0;
    while (i < val.len) {
        node.props.append(val[i]);
        i += 1;
    }
    node.props.append('"');
}

// 逐字节切片比较（与 checker.hc 同实现；== 对运行时堆子切片不可靠）
fn slice_eq(a: &[u8], b: &[u8]) bool {
    if (a.len != b.len) return false;
    var mut i: usize = 0;
    while (i < a.len) {
        if (a[i] != b[i]) return false;
        i += 1;
    }
    return true;
}

// 从 props 中提取属性值（key=value 格式，用 | 分隔；与 checker.hc 同实现）
fn get_prop(props: &[u8], key: &[u8]) ?&[u8] {
    var mut i: usize = 0;
    var n = props.len;
    while (i < n) {
        if (props[i] == '|') { i += 1; }
        if (i + key.len < n) {
            var mut match_key = true;
            var mut j: usize = 0;
            while (j < key.len) {
                if (props[i + j] != key[j]) { match_key = false; break; }
                j += 1;
            }
            if (match_key and props[i + key.len] == '=') {
                var mut val_start = i + key.len + 1;
                var mut skip_quote = false;
                if (val_start < n and props[val_start] == '"') {
                    skip_quote = true;
                    val_start += 1;
                }
                var mut val_end = val_start;
                while (val_end < n) {
                    if (skip_quote and props[val_end] == '"') { break; }
                    if (!skip_quote and props[val_end] == '|') { break; }
                    val_end += 1;
                }
                return props[val_start..val_end];
            }
        }
        while (i < n and props[i] != '|') { i += 1; }
    }
    return null;
}

// 检查属性是否存在
fn has_prop(props: &[u8], key: &[u8]) bool {
    var v = get_prop(props, key);
    return v != null;
}

fn append_int(v: i64, out: *mut Vec<u8>) void {
    var mut u = v;
    if (u < 0) {
        out.*.append('-');
        u = -u;
    }
    if (u == 0) {
        out.*.append('0');
        return;
    }
    var tmp = Vec<u8>.init(alloc);
    while (u > 0) {
        tmp.append(@intCast(u8, u % 10) + '0');
        u = u / 10;
    }
    var mut i: i64 = @intCast(i64, tmp.len) - 1;
    while (i >= 0) {
        out.*.append(tmp[@intCast(usize, i)]);
        i -= 1;
    }
}
// ============================================================
// 解析器（Parser）
// ============================================================

class Parser {
    tokens: Vec<Token>,
    mut pos: usize,
    n: usize,
    rev_kw_map: Map<&[u8], &[u8]>,

    fn peek(self: *mut Self) &[u8] {
        var tok = self.tokens[self.pos];
        return tok.kind;
    }

    fn peek_n(self: *mut Self, n: usize) &[u8] {
        var mut idx = self.pos + n;
        if (idx >= self.n) { idx = self.n - 1; }
        var tok = self.tokens[idx];
        return tok.kind;
    }

    // 判定 `Ident<` 是否为泛型实参（匹配 `>` 后跟 . ( { 时为真；否则视为小于号）
    // 遇语句边界（; { } and or if/while/for/return 等）即判定非泛型，避免跨语句误扫；
    // Shr（>>）在深度 ≥2 时视为嵌套泛型闭合
    fn generic_args_ahead(self: *mut Self) bool {
        var mut i: usize = self.pos + 1;
        var mut depth: usize = 1;
        while (i < self.n and depth > 0) {
            var k2 = self.tokens[i].kind;
            if (k2 == "Lt") { depth += 1; }
            else if (k2 == "Gt") { depth -= 1; }
            else if (k2 == "Shr") {
                if (depth < 2) { return false; }
                depth -= 2;
            }
            else if (k2 == "Semi" or k2 == "LBrace" or k2 == "RBrace" or k2 == "KwAnd" or k2 == "KwOr" or k2 == "KwIf" or k2 == "KwWhile" or k2 == "KwFor" or k2 == "KwReturn" or k2 == "KwFn" or k2 == "KwClass") { return false; }
            i += 1;
        }
        if (depth != 0) { return false; }
        if (i >= self.n) { return false; }
        var nk = self.tokens[i].kind;
        return nk == "Dot" or nk == "LParen" or nk == "LBrace";
    }

    fn peek_text(self: *mut Self) &[u8] {
        var tok = self.tokens[self.pos];
        return tok.text.as_slice();
    }

    fn at(self: *mut Self, kind: &[u8]) bool {
        return self.peek() == kind;
    }

    fn text_eq(self: *mut Self, s: &[u8]) bool {
        return self.peek_text() == s;
    }

    fn at_any(self: *mut Self, kinds: &[&[u8]]) bool {
        var mut i: usize = 0;
        while (i < kinds.len) {
            if (self.at(kinds[i])) return true;
            i += 1;
        }
        return false;
    }

    fn advance(self: *mut Self) Token {
        var t = self.tokens[self.pos];
        if (self.pos < self.n - 1) { self.pos += 1; }
        return t;
    }

    fn expect(self: *mut Self, kind: &[u8]) bool {
        if (self.at(kind)) {
            self.advance();
            return true;
        }
        return false;
    }

    fn expect_ident(self: *mut Self) &[u8] {
        if (self.at("Ident")) {
            var txt = self.peek_text();
            self.advance();
            return txt;
        }
        // 关键字也可作标识符（如 `type` 作字段名）
        var txt = self.peek_text();
        self.advance();
        return txt;
    }

    fn expect_name_or_keyword(self: *mut Self) &[u8] {
        var k = self.peek();
        if (k == "Ident") {
            var txt = self.peek_text();
            self.advance();
            return txt;
        }
        // 关键字可作点号字段名，用反向字典 O(1) 查找
        var txt = self.peek_text();
        if (self.rev_kw_map.contains(k)) {
            var name = self.rev_kw_map.get(k).?;
            self.advance();
            return name;
        }
        return txt;
    }

    // ---------- 程序入口 ----------

    fn parse_program(self: *mut Self) AstNode {
        var prog = make_node("Program");
        while (!self.at("Eof")) {
            var decl = self.parse_decl();
            node_add_child(&prog, decl);
        }
        return prog;
    }

    // ---------- 声明解析 ----------

    fn parse_decl(self: *mut Self) AstNode {
        // pub
        var mut is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        // export
        var mut is_export = false;
        if (self.at("KwExport")) { is_export = true; self.advance(); }
        // [pad] [align(T)] [Test]
        var traits = Vec<&[u8]>.init(alloc);
        while (self.at("LBracket")) {
            var t = self.parse_trait();
            if (t) |tr| { traits.append(tr); }
        }

        var k = self.peek();
        if (k == "KwGlobal") {
            self.advance();
            return self.parse_global(is_pub);
        }
        if (k == "KwConst") {
            self.advance();
            return self.parse_const(is_pub);
        }
        if (k == "KwAsync") {
            self.advance();
            self.expect("KwFn");
            return self.finish_fn_decl(traits, is_pub, true, is_export);
        }
        if (k == "KwExtern") {
            self.advance();
            return self.parse_extern_fn(is_pub);
        }
        if (k == "KwFn") {
            self.advance();
            return self.finish_fn_decl(traits, is_pub, false, is_export);
        }
        if (k == "KwClass" or k == "KwTree") {
            self.advance();
            return self.parse_class(is_pub);
        }
        if (k == "KwEnum") {
            self.advance();
            return self.parse_enum(is_pub);
        }
        if (k == "KwUnion") {
            self.advance();
            return self.parse_union(is_pub);
        }
        if (k == "KwInterface") {
            self.advance();
            return self.parse_interface(is_pub);
        }
        if (k == "KwNamespace") {
            self.advance();
            var name = self.expect_ident();
            self.expect("LBrace");
            var ns = make_node("Namespace");
            node_add_prop(&ns, "name", name);
            if (is_pub) { node_add_prop(&ns, "pub", "true"); }
            while (!self.at("RBrace") and !self.at("Eof")) {
                var d = self.parse_decl();
                node_add_child(&ns, d);
            }
            self.expect("RBrace");
            return ns;
        }
        if (k == "KwImport") {
            self.advance();
            var path = self.parse_path();
            // 选择集：import H.std.{io}（parse_path 已消费 `{` 前的点）
            var syms = Vec<u8>.init(alloc);
            var mut has_syms = false;
            if (self.at("LBrace")) {
                self.advance();
                has_syms = true;
                while (!self.at("RBrace") and !self.at("Eof")) {
                    var sname = self.expect_name_or_keyword();
                    // P7：记录选择符号（逗号分隔），供 interp 同目录文件加载
                    var mut j2: usize = 0;
                    while (j2 < sname.len) { syms.append(sname[j2]); j2 += 1; }
                    syms.append(',');
                    if (self.at("Ident") and self.peek_text() == "as") {
                        self.advance();
                        var _a = self.expect_ident();
                    }
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RBrace");
            }
            var mut alias: ?&[u8] = null;
            if (self.at("Ident") and self.peek_text() == "as") {
                self.advance();
                alias = self.expect_ident();
            }
            self.expect("Semi");
            var u = make_node("Import");
            node_add_prop(&u, "path", path);
            if (has_syms) {
                // 去掉尾逗号后存入 syms prop（空选择集不存）
                var body2 = syms.as_slice();
                if (body2.len > 0 and body2[body2.len - 1] == ',') {
                    node_add_prop(&u, "syms", body2[0..body2.len - 1]);
                } else if (body2.len > 0) {
                    node_add_prop(&u, "syms", body2);
                }
            }
            if (alias) |a| { node_add_prop(&u, "alias", a); }
            return u;
        }
        if (k == "KwScript") {
            self.advance();
            self.parse_block();
            var sc = make_node("Script");
            return sc;
        }
        if (k == "KwComptime") {
            self.advance();
            self.parse_block();
            var cp = make_node("Comptime");
            return cp;
        }
        // 未知声明 → 空节点并且推进当前 token 防止无限循环
        self.advance();
        return make_node("UnknownDecl");
    }

    fn parse_trait(self: *mut Self) ?&[u8] {
        self.expect("LBracket");
        var name = self.expect_ident();
        if (name == "continuous") { self.expect("RBracket"); return "continuous"; }
        if (name == "pad") { self.expect("RBracket"); return "pad"; }
        if (name == "module") { self.expect("RBracket"); return "module"; }
        if (name == "test") {
            if (self.at("LParen")) {
                self.advance();
                if (self.at("Str")) { self.advance(); }
                self.expect("RParen");
            }
            self.expect("RBracket");
            return "test";
        }
        if (name == "align") {
            self.expect("LParen");
            self.parse_type();
            self.expect("RParen");
            self.expect("RBracket");
            return "align";
        }
        self.expect("RBracket");
        return null;
    }

    fn parse_global(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var g = make_node("Global");
        node_add_prop(&g, "name", name);
        if (is_pub) { node_add_prop(&g, "pub", "true"); }
        if (self.at("Colon")) {
            self.advance();
            self.parse_type(); // ty info consumed
        }
        if (self.at("Eq")) {
            self.advance();
            self.parse_expr();
            node_add_prop(&g, "has_init", "true");
        }
        self.expect("Semi");
        return g;
    }

    fn parse_const(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var c = make_node("Const");
        node_add_prop(&c, "name", name);
        if (is_pub) { node_add_prop(&c, "pub", "true"); }
        //  error{...}
        if (self.at("Ident") and self.peek_text() == "error" and self.peek_n(1) == "LBrace") {
            self.advance();
            self.advance();
            while (!self.at("RBrace") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("RBrace");
            self.expect("Semi");
            return c;
        }
        self.expect("Eq");
        self.parse_expr();
        self.expect("Semi");
        return c;
    }

    fn finish_fn_decl(self: *mut Self, traits: Vec<&[u8]>, is_pub: bool, is_async: bool, is_export: bool) AstNode {
        var name = self.expect_ident();
        var f = make_node("Fn");
        node_add_prop(&f, "name", name);
        if (is_pub) { node_add_prop(&f, "pub", "true"); }
        if (is_async) { node_add_prop(&f, "async", "true"); }
        if (is_export) { node_add_prop(&f, "exported", "true"); }
        // 检查 [test] 特性
        var mut i: usize = 0;
        while (i < traits.len) {
            if (traits[i] == "test") {
                node_add_prop(&f, "test", "true");
            }
            i += 1;
        }
        // 泛型参数 <T>
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
            }
            self.expect("Gt");
        }
        // 参数 (params)
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var p = self.parse_param();
                node_add_child(&f, p);
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("RParen");
        // 返回类型
        if (self.at("Bang")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var mut ret_ty = self.peek_text();
                self.advance();
                self.consume_type_args();
                var r = make_node("ret:");
                var mut k: usize = 0;
                while (k < ret_ty.len) {
                    r.props.append(ret_ty[k]);
                    k += 1;
                }
                node_add_child(&f, r);
            } else {
                self.parse_type();
            }
        } else if (self.at("KwVoid") or self.at("Ident")) {
            var mut ret_ty = self.peek_text();
            // 关键字（如 void）的 text 为空，直接用关键字名
            if (ret_ty.len == 0) {
                if (self.at("KwVoid")) { ret_ty = "void"; }
            }
            self.advance();
            self.consume_type_args();
            var r = make_node("ret:");
            var mut k: usize = 0;
            while (k < ret_ty.len) {
                r.props.append(ret_ty[k]);
                k += 1;
            }
            node_add_child(&f, r);
        } else if (!self.at("LBrace") and !self.at("Semi") and !self.at("Eof")) {
            // 复杂类型（如 ?&[u8]、*[4]u8 等）：仅消费 token
            self.parse_type();
        }
        // where 子句
        if (self.at("KwWhere")) {
            self.advance();
            while (!self.at("LBrace") and !self.at("Semi") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        // 体部（extern fn 无 body）
        if (self.at("Semi")) {
            self.advance();
            node_add_prop(&f, "extern", "true");
            return f;
        }
        var body = self.parse_block();
        node_add_child(&f, body);
        return f;
    }

    fn parse_extern_fn(self: *mut Self, is_pub: bool) AstNode {
        self.expect("KwFn");
        var name = self.expect_ident();
        var f = make_node("Fn");
        node_add_prop(&f, "name", name);
        node_add_prop(&f, "extern", "true");
        if (is_pub) { node_add_prop(&f, "pub", "true"); }
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var p = self.parse_param();
                node_add_child(&f, p);
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("RParen");
        if (self.at("Bang")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var ret_ty = self.peek_text();
                self.advance();
                var r = make_node("ret:");
                var mut k: usize = 0;
                while (k < ret_ty.len) {
                    r.props.append(ret_ty[k]);
                    k += 1;
                }
                node_add_child(&f, r);
            } else {
                self.parse_type();
            }
        } else if (self.at("KwVoid") or self.at("Ident")) {
            var ret_ty = self.peek_text();
            self.advance();
            var r = make_node("ret:");
            var mut k: usize = 0;
            while (k < ret_ty.len) {
                r.props.append(ret_ty[k]);
                k += 1;
            }
            node_add_child(&f, r);
        }
        self.expect("Semi");
        return f;
    }

    fn parse_param(self: *mut Self) AstNode {
        // var/mut 前缀（如 var mut out: Vec<u8>）
        var mut is_mut = false;
        if (self.at("KwVar")) { self.advance(); is_mut = true; }
        if (self.at("KwMut")) { self.advance(); is_mut = true; }
        var name = self.expect_ident();
        self.expect("Colon");
        var p = make_node("Param");
        node_add_prop(&p, "name", name);
        if (is_mut) { node_add_prop(&p, "mut", "true"); }
        if (self.at("Ident") or self.at("KwVoid")) {
            var ty = self.peek_text();
            self.advance();
            if (ty.len > 0) {
                quoted_add_prop(&p, "ty", ty);
            } else {
                quoted_add_prop(&p, "ty", "void");
            }
            // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else {
            self.parse_type();
        }
        if (self.at("Eq")) {
            self.advance();
            self.parse_expr();
            node_add_prop(&p, "has_default", "true");
        }
        return p;
    }

    fn parse_class(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var cls = make_node("Class");
        node_add_prop(&cls, "name", name);
        if (is_pub) { node_add_prop(&cls, "pub", "true"); }
        // 接口
        if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        }
        // traits
        while (self.at("LBracket")) {
            self.parse_trait();
        }
        self.expect("LBrace");
        // 字段和方法
        while (!self.at("RBrace") and !self.at("Eof")) {
            if (self.at("KwFn") or self.at("LBracket") or (self.at("KwPub") and self.peek_n(1) == "KwFn")) {
                // 方法
                var m = self.parse_method(name);
                node_add_child(&cls, m);
            } else {
                // 字段
                var f = self.parse_field();
                node_add_child(&cls, f);
            }
        }
        self.expect("RBrace");
        return cls;
    }

    fn parse_field(self: *mut Self) AstNode {
        var mut is_fpub = false;
        if (self.at("KwPub")) { is_fpub = true; self.advance(); }
        var mut is_mut = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        var name = self.expect_ident();
        var f = make_node("FieldDecl");
        node_add_prop(&f, "name", name);
        if (is_mut) { node_add_prop(&f, "mut", "true"); }
        if (is_fpub) { node_add_prop(&f, "pub", "true"); }
        self.expect("Colon");
        // 简单 Ident 类型存 ty prop（对齐 Param 模式）；其余类型仅消费 token
        if (self.at("Ident")) {
            var ty = self.peek_text();
            self.advance();
            if (ty.len > 0) {
                quoted_add_prop(&f, "ty", ty);
            }
            // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else {
            self.parse_type();
        }
        // 分隔容错：逗号/分号均可
        if (self.at("Comma") or self.at("Semi")) { self.advance(); }
        return f;
    }

    fn parse_method(self: *mut Self, cls_name: &[u8]) AstNode {
        // traits
        var mut traits = Vec<&[u8]>.init(alloc);
        while (self.at("LBracket")) {
            var t = self.parse_trait();
            if (t) |tn| { traits.append(tn); }
        }
        var mut is_pub = false;
        if (self.at("KwPub")) { is_pub = true; self.advance(); }
        self.expect("KwFn");
        var f = self.finish_fn_decl(traits, is_pub, false, false);
        node_add_prop(&f, "method", cls_name);
        return f;
    }

    fn parse_enum(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var e = make_node("Enum");
        node_add_prop(&e, "name", name);
        if (is_pub) { node_add_prop(&e, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var vname = self.expect_ident();
            var v = make_node("Variant");
            node_add_prop(&v, "name", vname);
            if (self.at("LParen")) {
                self.advance();
                self.parse_type();
                self.expect("RParen");
            }
            node_add_child(&e, v);
            if (self.at("Comma")) { self.advance(); }
        }
        self.expect("RBrace");
        return e;
    }

    fn parse_union(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var u = make_node("Union");
        node_add_prop(&u, "name", name);
        if (is_pub) { node_add_prop(&u, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var fname = self.expect_ident();
            self.expect("Colon");
            self.parse_type();
            self.expect("Semi");
        }
        self.expect("RBrace");
        return u;
    }

    fn parse_interface(self: *mut Self, is_pub: bool) AstNode {
        var name = self.expect_ident();
        var iface = make_node("Interface");
        node_add_prop(&iface, "name", name);
        if (is_pub) { node_add_prop(&iface, "pub", "true"); }
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            self.expect("KwFn");
            var mname = self.expect_ident();
            self.expect("LParen");
            if (!self.at("RParen")) {
                while (true) {
                    var _ = self.parse_param();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RParen");
            if (self.at("Bang")) {
                self.advance();
                self.parse_type();
            } else if (self.at("KwVoid") or self.at("Ident")) {
                self.advance();
            }
            self.expect("Semi");
        }
        self.expect("RBrace");
        return iface;
    }

    fn parse_path(self: *mut Self) Vec<u8> {
        var parts = Vec<u8>.init(alloc);
        var first = self.expect_ident();
        var mut i: usize = 0;
        while (i < first.len) {
            parts.append(first[i]);
            i += 1;
        }
        while (self.at("Dot")) {
            self.advance();
            parts.append('.');
            var seg = self.expect_name_or_keyword();
            var mut j: usize = 0;
            while (j < seg.len) {
                parts.append(seg[j]);
                j += 1;
            }
        }
        return parts;
    }

    fn parse_import_path(self: *mut Self) Vec<u8> {
        return self.parse_path();
    }

    // ============================================================
    // 类型解析
    // ============================================================

    fn parse_type(self: *mut Self) void {
        // owned T
        if (self.at("KwOwned")) {
            self.advance();
            self.parse_type();
            return;
        }
        // *T / *mut T
        if (self.at("Star")) {
            self.advance();
            if (self.at("KwMut")) { self.advance(); }
            self.parse_type();
            return;
        }
        // &[T] / &mut [T] 或 &T
        if (self.at("Amp")) {
            self.advance();
            if (self.at("KwMut")) { self.advance(); }
            if (self.at("LBracket")) {
                self.advance();
                self.parse_type();
                self.expect("RBracket");
            } else {
                self.parse_type();
            }
            return;
        }
        // ?T
        if (self.at("Question")) {
            self.advance();
            self.parse_type();
            return;
        }
        // !T（anyerror）
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
            return;
        }
        // 基础类型
        self.parse_type_base();
        // E!T（命名错误集）
        if (self.at("Bang")) {
            self.advance();
            self.parse_type();
        }
    }

    fn parse_type_base(self: *mut Self) void {
        if (self.at("Ident")) {
            var name = self.peek_text();
            self.advance();
            // 泛型实参：Type(T1, T2)
            if (self.at("LParen")) {
                self.advance();
                while (!self.at("RParen") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RParen");
            }
            // 泛型实参：Type<T1, T2>
            if (self.at("Lt")) {
                self.advance();
                while (!self.at("Gt") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("Gt");
            }
        } else if (self.at("LBracket")) {
            // [N]T 定长数组
            self.advance();
            self.parse_expr();
            self.expect("RBracket");
            self.parse_type();
        } else if (self.at("LParen")) {
            // 元组
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        } else if (self.at("KwClass")) {
            // struct { ... } 类型字面量
            self.advance();
            self.expect("LBrace");
            if (!self.at("RBrace")) {
                while (true) {
                    self.expect_ident();
                    self.expect("Colon");
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBrace");
        } else {
            // 关键字作类型名（如 void, type 等）
            self.advance();
        }
    }

    // ============================================================
    // 语句解析
    // ============================================================

    fn parse_block(self: *mut Self) AstNode {
        var b = make_node("Block");
        // `{` 缺失时返回空块且不消费（防失控吞并；无括号体由 parse_block_or_stmt 包装）
        if (!self.at("LBrace")) { return b; }
        self.advance();
        while (!self.at("RBrace") and !self.at("Eof")) {
            var stmt = self.parse_stmt();
            node_add_child(&b, stmt);
        }
        self.expect("RBrace");
        return b;
    }

    // 块或单语句体：`if (c) stmt;` 无括号形式包装成 Block
    fn parse_block_or_stmt(self: *mut Self) AstNode {
        if (self.at("LBrace")) { return self.parse_block(); }
        var b = make_node("Block");
        var stmt = self.parse_stmt();
        node_add_child(&b, stmt);
        return b;
    }

    // 泛型实参仅消费：Type(T1,T2) / Type<T1,T2>
    fn consume_type_args(self: *mut Self) void {
        if (self.at("LParen")) {
            self.advance();
            while (!self.at("RParen") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("RParen");
        }
        if (self.at("Lt")) {
            self.advance();
            while (!self.at("Gt") and !self.at("Eof")) {
                self.parse_type();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
            self.expect("Gt");
        }
    }

    fn parse_stmt(self: *mut Self) AstNode {
        // 循环标签
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident")) { self.advance(); }
            if (self.at("KwWhile") or self.at("KwFor")) {
                // 标签后跟 while/for
            }
        }
        var k = self.peek();
        if (k == "LBrace") {
            return self.parse_block();
        }
        if (k == "Semi") {
            self.advance();
            return make_node("Empty");
        }
        if (k == "KwVar") {
            self.advance();
            return self.parse_var_decl();
        }
        if (k == "KwConst") {
            self.advance();
            var name = self.expect_ident();
            self.expect("Eq");
            self.parse_expr();
            self.expect("Semi");
            var c = make_node("ConstDecl");
            node_add_prop(&c, "name", name);
            return c;
        }
        if (k == "KwIf") {
            return self.parse_if_stmt();
        }
        if (k == "KwWhile") {
            return self.parse_while_stmt();
        }
        if (k == "KwFor") {
            return self.parse_for_stmt();
        }
        if (k == "KwSwitch") {
            return self.parse_switch_stmt();
        }
        if (k == "KwReturn") {
            self.advance();
            var r = make_node("Return");
            if (!self.at("Semi")) {
                var val = self.parse_expr();
                node_add_child(&r, val);
            }
            self.expect("Semi");
            return r;
        }
        if (k == "KwBreak") {
            self.advance();
            var b = make_node("Break");
            self.expect("Semi");
            return b;
        }
        if (k == "KwContinue") {
            self.advance();
            var c = make_node("Continue");
            self.expect("Semi");
            return c;
        }
        if (k == "KwDefer") {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Defer");
        }
        if (k == "KwErrdefer") {
            self.advance();
            self.parse_expr();
            self.expect("Semi");
            return make_node("Errdefer");
        }
        // 默认：表达式语句（含赋值：target = / += / -= / *= / /= value）
        var e = self.parse_expr();
        var ak = self.peek();
        if (ak == "Eq" or ak == "PlusEq" or ak == "MinusEq" or ak == "StarEq" or ak == "SlashEq") {
            self.advance();
            var rhs = self.parse_expr();
            var a = make_node("Assign");
            if (ak == "Eq") { node_add_prop(&a, "op", "Eq"); }
            else if (ak == "PlusEq") { node_add_prop(&a, "op", "PlusEq"); }
            else if (ak == "MinusEq") { node_add_prop(&a, "op", "MinusEq"); }
            else if (ak == "StarEq") { node_add_prop(&a, "op", "StarEq"); }
            else { node_add_prop(&a, "op", "SlashEq"); }
            node_add_child(&a, e);
            node_add_child(&a, rhs);
            self.expect("Semi");
            var aes = make_node("ExprStmt");
            node_add_child(&aes, a);
            return aes;
        }
        self.expect("Semi");
        var es = make_node("ExprStmt");
        node_add_child(&es, e);
        return es;
    }

    fn parse_var_decl(self: *mut Self) AstNode {
        var mut is_mut = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        var name = self.expect_ident();
        var v = make_node("VarDecl");
        node_add_prop(&v, "name", name);
        if (is_mut) { node_add_prop(&v, "mut", "true"); }
        if (self.at("Colon")) {
            self.advance();
            if (self.at("Ident") or self.at("KwVoid")) {
                var ty = self.peek_text();
                self.advance();
                if (ty.len > 0) {
                    quoted_add_prop(&v, "ty", ty);
                } else {
                    quoted_add_prop(&v, "ty", "void");
                }
            } else {
                self.parse_type();
            }
        }
        if (self.at("Eq")) {
            self.advance();
            var init = self.parse_expr();
            node_add_child(&v, init);
            node_add_prop(&v, "has_init", "true");
        }
        self.expect("Semi");
        return v;
    }

    fn parse_if_stmt(self: *mut Self) AstNode {
        self.advance();
        var ifn = make_node("If");
        self.expect("LParen");
        var cond = self.parse_expr();
        node_add_child(&ifn, cond);
        self.expect("RParen");
        // 载荷捕获（后置）：if (opt) |v| / if (x) |v| |e|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&ifn, "payload", cap);
            self.expect("Pipe");
        }
        if (self.at("Pipe")) {
            self.advance();
            var err = self.expect_ident();
            node_add_prop(&ifn, "payload_err", err);
            self.expect("Pipe");
        }
        var then_b = self.parse_block_or_stmt();
        node_add_child(&ifn, then_b);
        if (self.at("KwElse")) {
            self.advance();
            if (self.at("KwIf")) {
                var else_if = self.parse_if_stmt();
                node_add_child(&ifn, else_if);
            } else {
                var else_b = self.parse_block_or_stmt();
                node_add_child(&ifn, else_b);
            }
        }
        return ifn;
    }

    fn parse_while_stmt(self: *mut Self) AstNode {
        self.advance();
        var wn = make_node("While");
        self.expect("LParen");
        var cond = self.parse_expr();
        node_add_child(&wn, cond);
        self.expect("RParen");
        // 载荷捕获（后置）：while (it.next()) |x|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&wn, "payload", cap);
            self.expect("Pipe");
        }
        // step 子句
        if (self.at("Colon") and self.peek_n(1) == "LParen") {
            self.advance();
            self.expect("LParen");
            self.parse_expr();
            self.expect("RParen");
        }
        var body = self.parse_block_or_stmt();
        node_add_child(&wn, body);
        return wn;
    }

    fn parse_for_stmt(self: *mut Self) AstNode {
        self.advance();
        var for_node = make_node("For");
        self.expect("LParen");
        if (self.at("KwMut")) { self.advance(); }
        var iter = self.parse_expr();
        node_add_child(&for_node, iter);
        self.expect("RParen");
        // 载荷捕获（后置）：for (iter) |item|
        if (self.at("Pipe")) {
            self.advance();
            var cap = self.expect_ident();
            node_add_prop(&for_node, "payload", cap);
            self.expect("Pipe");
        }
        var body = self.parse_block_or_stmt();
        node_add_child(&for_node, body);
        return for_node;
    }

    fn parse_switch_stmt(self: *mut Self) AstNode {
        self.advance();
        var sn = make_node("Switch");
        self.expect("LParen");
        var subj = self.parse_expr();
        node_add_child(&sn, subj);
        self.expect("RParen");
        self.expect("LBrace");
        while (!self.at("RBrace") and !self.at("Eof")) {
            var arm = self.parse_switch_arm();
            node_add_child(&sn, arm);
        }
        self.expect("RBrace");
        return sn;
    }

    fn parse_switch_arm(self: *mut Self) AstNode {
        var arm = make_node("SwitchArm");
        // 模式列表（逗号分隔多个模式，直到 =>；对齐 Rust parser 多模式支持）
        while (!self.at("FatArrow") and !self.at("RBrace") and !self.at("Eof")) {
            var pat = self.parse_switch_pattern();
            node_add_child(&arm, pat);
            if (self.at("Comma")) { self.advance(); continue; }
            break;
        }
        self.expect("FatArrow");
        // 守卫
        if (self.at("KwIf")) {
            self.advance();
            self.parse_expr();
        }
        // 体（块或表达式）
        if (self.at("LBrace")) {
            var body = self.parse_block();
            node_add_child(&arm, body);
        } else {
            var e = self.parse_expr();
            var es = make_node("ExprStmt");
            node_add_child(&es, e);
            node_add_child(&arm, es);
        }
        if (self.at("Comma")) { self.advance(); }
        return arm;
    }

    fn parse_switch_pattern(self: *mut Self) AstNode {
        var p = make_node("Pattern");
        if (self.at("KwElse")) {
            self.advance();
            node_add_prop(&p, "else", "true");
        } else if (self.at("Dot")) {
            self.advance();
            var name = self.expect_ident();
            node_add_prop(&p, "error", name);
        } else if (self.at("Ident")) {
            var name = self.peek_text();
            self.advance();
            if (self.at("Dot")) {
                // error.NotFound
                self.advance();
                var err = self.expect_ident();
                node_add_prop(&p, "error", err);
            } else {
                node_add_prop(&p, "ident", name);
            }
        } else if (self.at("Int")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "int", txt);
        } else if (self.at("Float")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "float", txt);
        } else if (self.at("Str")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "str", txt);
        } else if (self.at("Char")) {
            var txt = self.peek_text();
            self.advance();
            node_add_prop(&p, "char", txt);
        } else {
            self.advance();
        }
        return p;
    }

    // ============================================================
    // 表达式解析（递归下降 + 优先级表）
    // ============================================================

    fn parse_expr(self: *mut Self) AstNode {
        return self.parse_or();
    }

    fn parse_or(self: *mut Self) AstNode {
        var mut l = self.parse_and();
        while (self.at("KwOr") or self.at("PipePipe")) {
            self.advance();
            var r = self.parse_and();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "Or");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_and(self: *mut Self) AstNode {
        var mut l = self.parse_range();
        while (self.at("KwAnd")) {
            self.advance();
            var r = self.parse_range();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "And");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_range(self: *mut Self) AstNode {
        var mut l = self.parse_comparison();
        if (self.at("DotDot")) {
            self.advance();
            var r = self.parse_comparison();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "Range");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_comparison(self: *mut Self) AstNode {
        var mut l = self.parse_bitor();
        var cmp_op = self.peek();
        if (cmp_op == "EqEq" or cmp_op == "Ne" or cmp_op == "Lt" or cmp_op == "Le" or cmp_op == "Gt" or cmp_op == "Ge") {
            self.advance();
            var r = self.parse_bitor();
            var b = make_node("Binary");
            if (cmp_op == "EqEq") { node_add_prop(&b, "op", "Eq"); }
            else { node_add_prop(&b, "op", cmp_op); }
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitor(self: *mut Self) AstNode {
        var mut l = self.parse_bitxor();
        while (self.at("Pipe")) {
            self.advance();
            var r = self.parse_bitxor();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitOr");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitxor(self: *mut Self) AstNode {
        var mut l = self.parse_bitand();
        while (self.at("Caret")) {
            self.advance();
            var r = self.parse_bitand();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitXor");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_bitand(self: *mut Self) AstNode {
        var mut l = self.parse_shift();
        while (self.at("Amp")) {
            self.advance();
            var r = self.parse_shift();
            var b = make_node("Binary");
            node_add_prop(&b, "op", "BitAnd");
            node_add_child(&b, l);
            node_add_child(&b, r);
            l = b;
        }
        return l;
    }

    fn parse_shift(self: *mut Self) AstNode {
        var mut l = self.parse_addsub();
        while (true) {
            var opname = self.peek();
            if (opname == "Shl" or opname == "Shr") {
                self.advance();
                var r = self.parse_addsub();
                var b = make_node("Binary");
                node_add_prop(&b, "op", opname);
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_addsub(self: *mut Self) AstNode {
        var mut l = self.parse_muldiv();
        while (true) {
            var opname = self.peek();
            if (opname == "Plus" or opname == "Minus") {
                self.advance();
                var r = self.parse_muldiv();
                var b = make_node("Binary");
                if (opname == "Plus") { node_add_prop(&b, "op", "Add"); }
                else { node_add_prop(&b, "op", "Sub"); }
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_muldiv(self: *mut Self) AstNode {
        var mut l = self.parse_unary();
        while (true) {
            var opname = self.peek();
            if (opname == "Star" or opname == "Slash" or opname == "Percent" or opname == "PercentPercent") {
                self.advance();
                var r = self.parse_unary();
                var b = make_node("Binary");
                if (opname == "Star") { node_add_prop(&b, "op", "Mul"); }
                else if (opname == "Slash") { node_add_prop(&b, "op", "Div"); }
                else if (opname == "Percent") { node_add_prop(&b, "op", "Mod"); }
                else { node_add_prop(&b, "op", "ModMod"); }
                node_add_child(&b, l);
                node_add_child(&b, r);
                l = b;
            } else { break; }
        }
        return l;
    }

    fn parse_unary(self: *mut Self) AstNode {
        var k = self.peek();
        if (k == "Minus") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Neg");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Bang") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "Not");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Tilde") {
            self.advance();
            var mut e = self.parse_unary();
            var u = make_node("Unary");
            node_add_prop(&u, "op", "BitNot");
            node_add_child(&u, e);
            return u;
        }
        if (k == "Amp") {
            self.advance();
            var mut is_mut = false;
            if (self.at("KwMut")) { is_mut = true; self.advance(); }
            var mut e = self.parse_unary();
            var a = make_node("AddrOf");
            if (is_mut) { node_add_prop(&a, "mut", "true"); }
            node_add_child(&a, e);
            return a;
        }
        if (k == "KwTry") {
            self.advance();
            var mut e = self.parse_unary();
            var t = make_node("Try");
            node_add_child(&t, e);
            return t;
        }
        if (k == "KwAwait") {
            self.advance();
            var mut e = self.parse_unary();
            var a = make_node("Await");
            node_add_child(&a, e);
            return a;
        }
        if (k == "KwSpawn") {
            self.advance();
            var args = self.parse_call_args();
            var c = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", "spawn");
            node_add_child(&c, callee);
            var mut i: usize = 0;
            while (i < args.len) {
                node_add_child(&c, args[i]);
                i += 1;
            }
            return c;
        }
        if (k == "KwMove") {
            self.advance();
            // 闭包
            if (self.at("Pipe") or (self.at("KwMut") and self.peek_n(1) == "Pipe")) {
                return self.parse_closure();
            }
            var mut e = self.parse_unary();
            var m = make_node("Move");
            node_add_child(&m, e);
            return m;
        }
        return self.parse_postfix();
    }

    fn parse_closure(self: *mut Self) AstNode {
        var c = make_node("Closure");
        var mut is_mut = false;
        var is_move = false;
        if (self.at("KwMut")) { is_mut = true; self.advance(); }
        self.expect("Pipe");
        if (!self.at("Pipe")) {
            while (true) {
                var p = self.expect_ident();
                if (self.at("Comma")) { self.advance(); }
                else { break; }
            }
        }
        self.expect("Pipe");
        // 体部
        if (self.at("LBrace")) {
            var body = self.parse_block();
            node_add_child(&c, body);
        } else {
            var mut e = self.parse_expr();
            var es = make_node("ExprStmt");
            node_add_child(&es, e);
            node_add_child(&c, es);
        }
        return c;
    }

    fn parse_postfix(self: *mut Self) AstNode {
        var mut e = self.parse_primary();
        while (true) {
            var kk = self.peek();
            if (kk == "Dot") {
                self.advance();
                if (self.at("Question")) {
                    // .? 链式解包
                    self.advance();
                    var u = make_node("Unwrap");
                    node_add_child(&u, e);
                    e = u;
                } else {
                    var field = self.expect_name_or_keyword();
                    if (self.at("LParen")) {
                        // 方法调用
                        var args = self.parse_call_args();
                        var call = make_node("Call");
                        var fe = make_node("Field");
                        node_add_prop(&fe, "field", field);
                        node_add_child(&fe, e);
                        node_add_child(&call, fe);
                        var mut i: usize = 0;
                        while (i < args.len) {
                            node_add_child(&call, args[i]);
                            i += 1;
                        }
                        e = call;
                    } else {
                        var fe = make_node("Field");
                        node_add_prop(&fe, "field", field);
                        node_add_child(&fe, e);
                        e = fe;
                    }
                }
            } else if (kk == "LBracket") {
                self.advance();
                var idx = self.parse_expr();
                self.expect("RBracket");
                var ie = make_node("Index");
                node_add_child(&ie, e);
                node_add_child(&ie, idx);
                e = ie;
            } else if (kk == "DotStar") {
                self.advance();
                var d = make_node("Deref");
                node_add_child(&d, e);
                e = d;
            } else if (kk == "Question") {
                // 后缀 ? 解包
                self.advance();
                var u = make_node("Unwrap");
                node_add_child(&u, e);
                e = u;
            } else if (kk == "LParen") {
                var args = self.parse_call_args();
                var call = make_node("Call");
                node_add_child(&call, e);
                var mut i: usize = 0;
                while (i < args.len) {
                    node_add_child(&call, args[i]);
                    i += 1;
                }
                e = call;
                // 泛型字面量
                // 泛型字面量：Pair<i32>{...}
                if (self.at("LBrace")) {
                    // 简单处理：跳过字面量字段
                    self.advance();
                    if (!self.at("RBrace")) {
                        while (true) {
                            self.expect_ident();
                            self.expect("Eq");
                            self.parse_expr();
                            if (self.at("Comma")) { self.advance(); }
                            else { break; }
                        }
                    }
                    self.expect("RBrace");
                }
            } else if (kk == "KwOrelse") {
                self.advance();
                var r = self.parse_expr();
                var orelse_node = make_node("Orelse");
                node_add_child(&orelse_node, e);
                node_add_child(&orelse_node, r);
                e = orelse_node;
            } else if (kk == "KwCatch") {
                self.advance();
                var c = make_node("Catch");
                node_add_child(&c, e);
                if (self.at("Pipe")) {
                    self.advance();
                    var name = self.expect_ident();
                    self.expect("Pipe");
                    var body = self.parse_block();
                    var bnode = make_node("Bind");
                    node_add_prop(&bnode, "name", name);
                    node_add_child(&bnode, body);
                    node_add_child(&c, bnode);
                } else {
                    var d = self.parse_expr();
                    var dnode = make_node("Default");
                    node_add_child(&dnode, d);
                    node_add_child(&c, dnode);
                }
                e = c;
            } else {
                break;
            }
        }
        return e;
    }

    fn parse_call_args(self: *mut Self) Vec<AstNode> {
        var args = Vec<AstNode>.init(alloc);
        self.expect("LParen");
        if (!self.at("RParen")) {
            while (true) {
                var a = self.parse_expr();
                args.append(a);
                if (self.at("Comma")) {
                    self.advance();
                    if (self.at("RParen")) { break; }
                } else { break; }
            }
        }
        self.expect("RParen");
        return args;
    }

    fn parse_primary(self: *mut Self) AstNode {
        var k = self.peek();
        // 闭包
        if (k == "Pipe" or (k == "KwMut" and self.peek_n(1) == "Pipe")) {
            return self.parse_closure();
        }
        // 推断枚举值 .variant
        if (k == "Dot") {
            self.advance();
            var variant = self.expect_name_or_keyword();
            var d = make_node("Dot");
            node_add_prop(&d, "field", variant);
            return d;
        }
        // @内建
        if (k == "AtBuiltin") {
            var txt = self.peek_text();
            self.advance();
            var args = self.parse_call_args();
            var call = make_node("Call");
            var callee = make_node("Ident");
            node_add_prop(&callee, "name", txt[0..txt.len]);
            node_add_child(&call, callee);
            var mut i: usize = 0;
            while (i < args.len) {
                node_add_child(&call, args[i]);
                i += 1;
            }
            return call;
        }
        // struct { ... } 类型字面量
        if (k == "KwClass") {
            self.advance();
            self.expect("LBrace");
            var st = make_node("StructType");
            if (!self.at("RBrace")) {
                while (true) {
                    var name = self.expect_ident();
                    if (self.at("Colon")) {
                        self.advance();
                        self.parse_type();
                    } else if (self.at("Eq")) {
                        self.advance();
                        self.parse_expr();
                    }
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBrace");
            return st;
        }
        // 字面量
        if (k == "Int") {
            var txt = self.peek_text();
            self.advance();
            var mut n = make_node("IntLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (k == "Float") {
            var txt = self.peek_text();
            self.advance();
            var mut n = make_node("FloatLit");
            node_add_prop(&n, "text", txt[0..txt.len]);
            return n;
        }
        if (k == "Str") {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            return s;
        }
        if (k == "RawStr") {
            var txt = self.peek_text();
            self.advance();
            var s = make_node("StrLit");
            node_add_prop(&s, "value", txt[0..txt.len]);
            node_add_prop(&s, "raw", "true");
            return s;
        }
        if (k == "Char") {
            var txt = self.peek_text();
            self.advance();
            var c = make_node("CharLit");
            // 值以十进制文本存储——原始字节可能是 " 或 | 等属性分隔符，会破坏 |key=value 编码
            //（get_prop 的引号剥离/| 截断曾使 '"' 与 '|' 字面量求值为 0，字符串分派失效）
            if (txt.len > 0) {
                var dec = Vec<u8>.init(alloc);
                append_int(@intCast(i64, txt[0]), &mut dec);
                node_add_prop(&c, "value", dec.as_slice());
            }
            return c;
        }
        if (k == "KwTrue") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "true");
            return b;
        }
        if (k == "KwFalse") {
            self.advance();
            var b = make_node("BoolLit");
            node_add_prop(&b, "value", "false");
            return b;
        }
        if (k == "KwNull") {
            self.advance();
            return make_node("NullLit");
        }
        if (k == "KwVoid") {
            self.advance();
            return make_node("VoidLit");
        }
        // 标识符
        if (k == "Ident") {
            var name = self.peek_text();
            self.advance();
            // 枚举常量 error.NotFound
            if (name == "error" and self.at("Dot")) {
                self.advance();
                var err = self.expect_ident();
                var e = make_node("ErrorLit");
                node_add_prop(&e, "name", err);
                return e;
            }
            var id = make_node("Ident");
            node_add_prop(&id, "name", name[0..name.len]);
            // 类字面量：Type{field = val, ...}
            if (self.at("LBrace")) {
                self.advance();
                var cl = make_node("ClassLit");
                node_add_prop(&cl, "name", name[0..name.len]);
                while (!self.at("RBrace") and !self.at("Eof")) {
                    var fname = self.expect_name_or_keyword();
                    var fi = make_node("FieldInit");
                    node_add_prop(&fi, "name", fname);
                    if (self.at("Eq")) {
                        self.advance();
                        var vexpr = self.parse_expr();
                        node_add_child(&fi, vexpr);
                    }
                    node_add_child(&cl, fi);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                self.expect("RBrace");
                return cl;
            }
            // 泛型类型表达式：Vec<u8>.init(...) / Vec<Vec<u8>>.init（仅当匹配 `>` 后跟 . ( { 时消费，避免误吞小于号）
            if (self.at("Lt") and self.generic_args_ahead()) {
                self.advance();
                while (!self.at("Gt") and !self.at("Shr") and !self.at("Eof")) {
                    self.parse_type();
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
                if (self.at("Shr")) { self.advance(); }
                else { self.expect("Gt"); }
            }
            // 它后面可能跟泛型实参：Type(T1)
            if (self.at("LParen") and self.peek_n(1) != "RParen" and self.peek_n(1) != "Star" and self.peek_n(1) != "Slash" and self.peek_n(1) != "Plus" and self.peek_n(1) != "Minus") {
                // 可能是类型构造或函数调用，由 parse_postfix 处理
                // 但这里不做超前判断，交给调用者
            }
            return id;
        }
        // 错误字面量
        if (k == "KwScript") {
            self.advance();
            self.parse_block();
            return make_node("Script");
        }
        // 块表达式
        if (k == "LBrace") {
            return self.parse_block();
        }
        // 元组/括号表达式
        if (k == "LParen") {
            self.advance();
            var e = self.parse_expr();
            if (self.at("Comma")) {
                // 元组
                var t = make_node("TupleLit");
                node_add_child(&t, e);
                while (self.at("Comma")) {
                    self.advance();
                    var el = self.parse_expr();
                    node_add_child(&t, el);
                }
                self.expect("RParen");
                return t;
            }
            self.expect("RParen");
            return e;
        }
        // 数组字面量
        if (k == "LBracket") {
            self.advance();
            var arr = make_node("ArrayLit");
            if (!self.at("RBracket")) {
                while (true) {
                    var e = self.parse_expr();
                    node_add_child(&arr, e);
                    if (self.at("Comma")) { self.advance(); }
                    else { break; }
                }
            }
            self.expect("RBracket");
            return arr;
        }
        // 错误：跳过
        self.advance();
        return make_node("Unknown");
    }
}

// AST 输出（dump 函数，与 Rust `hc parse` 格式一致）
// ============================================================

class AstDumper {
    mut buf: Vec<u8>,

    fn dump(self: *mut Self, node: AstNode, depth: i32) void {
        var mut i = 0;
        while (i < depth * 2) {
            self.buf.append(' ');
            i += 1;
        }
        var mut kind_str = node.kind;
        // Handle ret: nodes specially
        if (kind_str == "ret:") {
            self.buf.append('r'); self.buf.append('e'); self.buf.append('t'); self.buf.append(':'); self.buf.append(' ');
            if (node.props.len > 0) {
                var s = node.props.as_slice();
                self.buf.append('"');
                var mut j: usize = 0;
                while (j < s.len) {
                    self.buf.append(s[j]);
                    j += 1;
                }
                self.buf.append('"');
            }
            self.buf.append('\n');
            return;
        }
        // kind
        var mut j: usize = 0;
        while (j < kind_str.len) {
            self.buf.append(kind_str[j]);
            j += 1;
        }
        // props
        if (node.props.len > 0) {
            var s = node.props.as_slice();
            j = 0;
            while (j < s.len) {
                self.buf.append(s[j]);
                j += 1;
            }
        }
        self.buf.append('\n');
        // children
        var mut ci: usize = 0;
        while (ci < node.children.len) {
            self.dump(node.children[ci], depth + 1);
            ci += 1;
        }
    }
}

