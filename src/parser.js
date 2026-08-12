// H 语言语法分析器（从原型 #3-#5 提取并修复）
// 递归下降解析器；AST 节点带 loc；parse(src) 返回 Program

const { lex } = require("./lexer");

class Parser {
  constructor(toks) { this.t = toks; this.p = 0; this.noConstruct = false; this.noRange = false; }
  peek(off) { return this.t[Math.min(this.p + (off || 0), this.t.length - 1)]; }
  next() { return this.t[this.p++]; }
  skipNL() { while (this.peek().kind === "NEWLINE") this.p++; }
  check(kind, value) { const tk = this.peek(); return tk.kind === kind && (value === undefined || tk.value === value); }
  match(kind, value) { if (this.check(kind, value)) { this.p++; return true; } return false; }
  expect(kind, value, what) {
    const tk = this.peek();
    if (tk.kind === kind && (value === undefined || tk.value === value)) { this.p++; return tk; }
    throw { parse: true, msg: "期望 " + (what || (value !== undefined ? "'" + value + "'" : kind)) + "，但遇到 '" + (tk.value || tk.kind) + "'", line: tk.line, col: tk.col };
  }
  err(msg, tk) { throw { parse: true, msg, line: tk.line, col: tk.col }; }

  parseProgram() {
    const decls = [];
    while (true) { this.skipNL(); if (this.check("EOF")) break; decls.push(this.parseTopLevel()); }
    return { type: "Program", decls };
  }
  parseTopLevel() {
    if (this.match("KEYWORD", "struct")) return this.parseStruct();
    if (this.match("KEYWORD", "class")) return this.parseClass();
    if (this.match("KEYWORD", "enum")) return this.parseEnum();
    if (this.match("KEYWORD", "interface")) return this.parseInterface();
    if (this.match("KEYWORD", "fun")) return this.parseFun(true);
    if (this.match("KEYWORD", "global")) return this.parseGlobal();
    if (this.match("KEYWORD", "spawn")) { const e = this.parseExpr(); this.optSemi(); return { type: "SpawnStmt", callee: e }; }
    return this.parseStmt();
  }
  parseStruct() {
    const tk = this.expect("IDENT", undefined, "结构体名"); const name = tk.value;
    this.expect("OP", "{");
    const fields = [];
    while (true) { this.skipNL(); if (this.match("OP", "}")) break; fields.push(this.parseField()); this.fieldSep(); }
    return { type: "StructDecl", name, fields, loc: { line: tk.line, col: tk.col } };
  }
  parseField() {
    let isMut = false;
    if (this.match("KEYWORD", "mut")) isMut = true;
    const tk = this.expect("IDENT", undefined, "字段名"); const name = tk.value;
    this.expect("OP", ":", "':'");
    return { type: "FieldDecl", name, fieldType: this.parseType(), isMut, loc: { line: tk.line, col: tk.col } };
  }
  fieldSep() {
    if (this.match("OP", ";") || this.match("OP", ",")) return;
    if (this.peek().kind === "NEWLINE") { this.skipNL(); return; }
    if (this.peek().value === "}") return;
    const tk = this.peek();
    this.err("期望字段分隔（换行/分号）或 '}'，但遇到 '" + (tk.value || tk.kind) + "'", tk);
  }
  parseClass() {
    const tk = this.expect("IDENT", undefined, "类名"); const name = tk.value;
    const interfaces = [];
    if (this.match("OP", ":")) { do { interfaces.push(this.expect("IDENT", undefined, "接口名").value); } while (this.match("OP", ",")); }
    this.expect("OP", "{");
    const imports = [], hides = [], aliases = [], fields = [], methods = [];
    while (true) {
      this.skipNL();
      if (this.match("OP", "}")) break;
      if (this.match("KEYWORD", "import")) { imports.push({ type: "ImportSpec", name: this.expect("IDENT", undefined, "类名").value }); continue; }
      if (this.match("KEYWORD", "hide")) { hides.push({ type: "HideSpec", path: this.parsePath() }); continue; }
      if (this.match("KEYWORD", "alias")) {
        const alias = this.expect("IDENT", undefined, "别名").value;
        this.expect("OP", "=", "'='");
        aliases.push({ type: "AliasSpec", alias, path: this.parsePath() });
        continue;
      }
      if (this.peek().kind === "KEYWORD" && this.peek().value === "fun") { this.next(); methods.push(this.parseFun(false)); continue; }
      fields.push(this.parseField());
      this.fieldSep();
    }
    return { type: "ClassDecl", name, interfaces, imports, hides, aliases, fields, methods, loc: { line: tk.line, col: tk.col } };
  }
  parsePath() {
    const parts = [this.expect("IDENT", undefined, "名称").value];
    while (this.match("OP", "::")) parts.push(this.expect("IDENT", undefined, "名称").value);
    return { type: "Path", parts };
  }
  parseEnum() {
    const tk = this.expect("IDENT", undefined, "枚举名"); const name = tk.value;
    this.expect("OP", "{");
    const variants = [];
    this.skipNL();
    if (!this.check("OP", "}")) { do { variants.push(this.expect("IDENT", undefined, "变体名").value); this.skipNL(); } while (this.match("OP", ",") || this.match("NEWLINE")); }
    this.expect("OP", "}");
    return { type: "EnumDecl", name, variants, loc: { line: tk.line, col: tk.col } };
  }
  parseInterface() {
    const tk = this.expect("IDENT", undefined, "接口名"); const name = tk.value;
    this.expect("OP", "{");
    const methods = [];
    while (true) {
      this.skipNL();
      if (this.match("OP", "}")) break;
      if (this.match("KEYWORD", "fun")) methods.push(this.parseFunSig());
      else { const t2 = this.peek(); this.err("接口体内只允许 fun 签名，但遇到 '" + (t2.value || t2.kind) + "'", t2); }
    }
    return { type: "InterfaceDecl", name, methods, loc: { line: tk.line, col: tk.col } };
  }
  parseFunSig() {
    const tk = this.expect("IDENT", undefined, "函数名"); const name = tk.value;
    const params = this.parseParams();
    const ret = this.peek().value === "->" ? this.parseRet() : null;
    return { type: "FunSig", name, params, ret, loc: { line: tk.line, col: tk.col } };
  }
  parseFun() {
    const tk = this.expect("IDENT", undefined, "函数名"); const name = tk.value;
    const params = this.parseParams();
    const ret = this.peek().value === "->" ? this.parseRet() : null;
    const body = this.parseBlock();
    return { type: "FunDecl", name, params, ret, body, loc: { line: tk.line, col: tk.col } };
  }
  parseParams() {
    this.expect("OP", "(");
    const params = [];
    this.skipNL();
    if (!this.check("OP", ")")) {
      do {
        this.skipNL();
        const ptk = this.expect("IDENT", undefined, "参数名"); const pname = ptk.value;
        this.expect("OP", ":");
        let kind = "val";
        if (this.match("KEYWORD", "ref")) kind = "ref";
        else if (this.match("KEYWORD", "move")) kind = "move";
        params.push({ type: "Param", name: pname, kind, ptype: this.parseType(), loc: { line: ptk.line, col: ptk.col } });
        this.skipNL();
      } while (this.match("OP", ","));
    }
    this.expect("OP", ")");
    return params;
  }
  parseRet() {
    this.expect("OP", "->", "'->'");
    let kind = "val";
    if (this.match("KEYWORD", "error")) kind = "error";
    else if (this.match("KEYWORD", "move")) kind = "move";
    else if (this.match("KEYWORD", "ref")) kind = "ref";
    return { type: "RetType", kind, rtype: this.parseType() };
  }
  parseGlobal() {
    const tk = this.expect("IDENT", undefined, "全局名"); const name = tk.value;
    this.expect("OP", ":");
    const gtype = this.parseType();
    let init = null;
    if (this.match("OP", "=")) init = this.parseExpr();
    this.optSemi();
    return { type: "GlobalDecl", name, gtype, init, loc: { line: tk.line, col: tk.col } };
  }
  parseType() {
    if (this.check("OP", "[")) {
      this.next();
      if (this.match("OP", "]")) { const elem = this.parseType(); return { type: "SliceType", elem }; }   // []T 切片（借用视图）
      const elem = this.parseType();
      this.expect("OP", "]", "']'");
      return { type: "ArrayType", elem };
    }
    if (this.check("OP", "(")) {
      // 元组类型：(T1, T2) / (x: T1, y: T2) / ()
      this.next();
      const items = [];
      this.skipNL();
      if (this.match("OP", ")")) return { type: "TupleType", named: false, items: [] };
      let named = this.check("IDENT") && this.peek(1).value === ":";
      while (true) {
        this.skipNL();
        if (named) {
          const fname = this.expect("IDENT", undefined, "字段名").value;
          this.expect("OP", ":", "':'");
          items.push({ name: fname, type: this.parseType() });
        } else {
          items.push({ name: null, type: this.parseType() });
        }
        this.skipNL();
        if (this.match("OP", ",")) { if (this.check("OP", ")")) break; continue; }
        break;
      }
      this.expect("OP", ")");
      return { type: "TupleType", named, items };
    }
    let mutable = false;
    if (this.match("KEYWORD", "ref")) mutable = true;
    const name = this.expect("IDENT", undefined, "类型名").value;
    if (this.match("OP", "<")) {
      const args = [];
      do { args.push(this.parseType()); } while (this.match("OP", ","));
      this.expect("OP", ">", "'>'");
      return { type: "GenericType", name, args, mutable };
    }
    return { type: "NamedType", name, mutable };
  }
  parseBlock() {
    this.expect("OP", "{", "'{'");
    const stmts = [];
    while (true) {
      this.skipNL();
      if (this.match("OP", "}")) break;
      if (this.check("EOF")) this.err("块未闭合（缺少 '}'）", this.peek());
      stmts.push(this.parseStmt());
    }
    return { type: "Block", stmts };
  }
  optSemi() { this.match("OP", ";"); this.skipNL(); }
  parseStmt() {
    if (this.match("KEYWORD", "for")) {
      const tk = this.t[this.p - 1];
      const id = this.expect("IDENT", undefined, "循环变量");
      this.expect("KEYWORD", "in", "'in'");
      const saved = this.noConstruct;
      this.noConstruct = true;             // 区间里的 { 是循环体，不是构造字面量
      const range = this.parseExpr();
      this.noConstruct = saved;
      const body = this.parseBlock();
      return { type: "ForStmt", varName: id.value, range, body, loc: { line: tk.line, col: tk.col } };
    }
    if (this.match("KEYWORD", "while")) {
      const tk = this.t[this.p - 1];
      const saved = this.noConstruct;
      this.noConstruct = true;             // 条件里的 IDENT{ 是块边界，不是构造字面量
      const cond = this.parseExpr();
      this.noConstruct = saved;
      const body = this.parseBlock();
      return { type: "WhileStmt", cond, body, loc: { line: tk.line, col: tk.col } };
    }
    if (this.match("KEYWORD", "break")) { const tk = this.t[this.p - 1]; this.optSemi(); return { type: "BreakStmt", loc: { line: tk.line, col: tk.col } }; }
    if (this.match("KEYWORD", "continue")) { const tk = this.t[this.p - 1]; this.optSemi(); return { type: "ContinueStmt", loc: { line: tk.line, col: tk.col } }; }
    if (this.match("KEYWORD", "return")) {
      const tk = this.t[this.p - 1];
      let expr = null;
      if (!this.check("OP", ";") && !this.check("OP", "}") && this.peek().kind !== "NEWLINE") expr = this.parseExpr();
      this.optSemi();
      return { type: "ReturnStmt", expr, loc: { line: tk.line, col: tk.col } };
    }
    if (this.match("KEYWORD", "yield")) { this.optSemi(); return { type: "YieldStmt" }; }
    if (this.match("KEYWORD", "spawn")) { const callee = this.parseExpr(); this.optSemi(); return { type: "SpawnStmt", callee }; }
    if (this.check("KEYWORD", "if")) {
      this.next();
      const saved = this.noConstruct;
      this.noConstruct = true;             // 条件里的 IDENT{ 是块边界，不是构造字面量
      const cond = this.parseExpr();
      this.noConstruct = saved;
      const then = this.parseBlock();
      let els = null;
      this.skipNL();
      if (this.match("KEYWORD", "else")) { if (this.check("KEYWORD", "if")) els = this.parseStmt(); else els = this.parseBlock(); }
      return { type: "IfStmt", cond, then, els };
    }
    if (this.match("OP", "{")) { this.p--; return this.parseBlock(); }
    if (this.match("KEYWORD", "mut") || this.match("KEYWORD", "ref")) {
      const kind = this.t[this.p - 1].value;
      return this.parseVarDecl(kind);
    }
    if (this.check("IDENT")) {
      const save = this.p;
      const id = this.next();
      this.skipNL();
      if (this.match("OP", "=")) {
        const init = this.parseExpr();
        this.optSemi();
        return { type: "VarDecl", name: id.value, kind: "val", annotation: null, init, loc: { line: id.line, col: id.col } };
      }
      if (this.match("OP", ":")) {
        const ann = this.parseType();
        let init = null;
        if (this.match("OP", "=")) init = this.parseExpr();
        this.optSemi();
        return { type: "VarDecl", name: id.value, kind: "val", annotation: ann, init, loc: { line: id.line, col: id.col } };
      }
      this.p = save;
    }
    const e = this.parseExpr();
    this.optSemi();
    return { type: "ExprStmt", expr: e };
  }
  parseVarDecl(kind) {
    const tk = this.expect("IDENT", undefined, "变量名"); const name = tk.value;
    let ann = null;
    if (this.match("OP", ":")) ann = this.parseType();
    let init = null;
    if (this.match("OP", "=")) init = this.parseExpr();
    this.optSemi();
    return { type: "VarDecl", name, kind, annotation: ann, init, loc: { line: tk.line, col: tk.col } };
  }
  parseMatch(mk) {
    const saved = this.noConstruct;
    this.noConstruct = true;             // target 表达式里的 { 是 match 体，不是构造字面量
    const target = this.parseExpr();
    this.noConstruct = saved;
    this.expect("OP", "{", "'{");
    const arms = [];
    while (true) {
      this.skipNL();
      if (this.match("OP", "}")) break;
      this.skipNL();
      const vt = this.expect("IDENT", undefined, "变体名");
      this.expect("OP", "=>", "'=>'");
      const expr = this.parseExpr();
      arms.push({ variant: vt.value, expr, loc: { line: vt.line, col: vt.col } });
      this.skipNL();
      if (!this.match("OP", ",")) { /* 换行分隔 */ }
    }
    return { type: "MatchExpr", target, arms, loc: { line: mk.line, col: mk.col } };
  }
  parseExpr() { return this.parseAssign(); }
  parseAssign() {
    const left = this.parseBinary(1);
    if (this.check("OP", "=") || this.check("OP", "+=") || this.check("OP", "-=") || this.check("OP", "*=") || this.check("OP", "/=")) {
      const tk = this.next();
      return { type: "AssignExpr", op: tk.value, left, right: this.parseAssign(), loc: { line: tk.line, col: tk.col } };
    }
    return left;
  }
  parseBinary(minPrec) {
    const PREC = { "||": 1, "&&": 2, "==": 3, "!=": 3, "<": 4, "<=": 4, ">": 4, ">=": 4, "+": 5, "-": 5, "*": 6, "/": 6, "%": 6 };
    let left = this.parseUnary();
    while (true) {
      const tk = this.peek();
      if (tk.kind !== "OP") break;
      const prec = PREC[tk.value];
      if (prec === undefined || prec < minPrec) break;
      this.next();
      left = { type: "BinExpr", op: tk.value, left, right: this.parseBinary(prec + 1) };
    }
    return left;
  }
  parseUnary() {
    if (this.check("OP", "!") || this.check("OP", "-")) { const op = this.next().value; return { type: "UnaryExpr", op, operand: this.parseUnary() }; }
    if (this.match("KEYWORD", "move")) return { type: "MoveExpr", expr: this.parseUnary() };
    return this.parsePostfix();
  }
  parsePostfix() {
    let e = this.parsePrimary();
    while (true) {
      if (this.match("OP", ".")) {
        let prop;
        if (this.check("IDENT")) prop = this.next().value;
        else if (this.check("NUMBER")) prop = String(this.next().value);   // 元组 .0/.1
        else this.err("期望属性名", this.peek());
        e = { type: "MemberExpr", obj: e, prop };
      }
      else if (this.check("OP", "(")) { e = { type: "CallExpr", callee: e, args: this.parseArgs() }; }
      else if (this.check("OP", "[")) {
        this.next();
        const saved = this.noRange;
        this.noRange = true;             // 索引内 1..3 是区间分界，裸区间不在此处解析
        if (this.check("OP", "]")) { this.next(); e = { type: "RangeExpr", obj: e, start: null, end: null }; }
        else if (this.check("OP", "..")) {
          this.next();
          let end = null;
          if (!this.check("OP", "]")) end = this.parseExpr();
          this.expect("OP", "]", "']'");
          e = { type: "RangeExpr", obj: e, start: null, end };
        }
        else {
          const start = this.parseExpr();
          if (this.match("OP", "..")) {
            let end = null;
            if (!this.check("OP", "]")) end = this.parseExpr();
            this.expect("OP", "]", "']'");
            e = { type: "RangeExpr", obj: e, start, end };
          } else {
            this.expect("OP", "]", "']'");
            e = { type: "IndexExpr", obj: e, index: start };
          }
        }
        this.noRange = saved;
      }
      else if (this.check("OP", "..") && !this.noRange) {
        // 裸数字区间 0..n（for 循环用；切片区间仍走上面的 "[" 分支）
        this.next();
        const end = this.parseExpr();
        e = { type: "RangeExpr", obj: e, start: null, end };
      }
      else break;
    }
    return e;
  }
  parseArgs() {
    this.expect("OP", "(");
    const args = [];
    this.skipNL();
    if (!this.check("OP", ")")) { do { this.skipNL(); args.push(this.parseExpr()); this.skipNL(); } while (this.match("OP", ",")); }
    this.expect("OP", ")");
    return args;
  }
  parsePrimary() {
    const tk = this.peek();
    if (tk.kind === "KEYWORD" && tk.value === "match") { this.next(); return this.parseMatch(tk); }
    if (tk.kind === "NUMBER") { this.next(); return { type: "Literal", kind: tk.float ? "float" : "number", value: tk.value, loc: { line: tk.line, col: tk.col } }; }
    if (tk.kind === "STRING") { this.next(); return { type: "Literal", kind: "string", value: tk.value, loc: { line: tk.line, col: tk.col } }; }
    if (tk.kind === "KEYWORD" && (tk.value === "true" || tk.value === "false")) { this.next(); return { type: "Literal", kind: "bool", value: tk.value === "true", loc: { line: tk.line, col: tk.col } }; }
    if (tk.kind === "KEYWORD" && tk.value === "error") {
      this.next();
      this.expect("OP", ".", "'.'");
      return { type: "ErrorLit", name: this.expect("IDENT", undefined, "错误名").value, loc: { line: tk.line, col: tk.col } };
    }
    if (this.check("OP", "(")) {
      // 元组字面量：(v1, v2) / (x: v1, y: v2) / () / (v,)；单元素无逗号 = 分组 (v)
      this.next();
      this.skipNL();
      const tl = { line: tk.line, col: tk.col };
      if (this.match("OP", ")")) return { type: "TupleLit", named: false, items: [], loc: tl };
      let named = this.check("IDENT") && this.peek(1).value === ":";
      const items = [];
      let hadComma = false;
      while (true) {
        this.skipNL();
        let mut = false;
        if (this.match("KEYWORD", "mut")) mut = true;
        if (named) {
          const fname = this.expect("IDENT", undefined, "字段名").value;
          this.expect("OP", ":", "':'");
          items.push({ name: fname, expr: this.parseExpr(), mut });
        } else {
          items.push({ name: null, expr: this.parseExpr(), mut });
        }
        this.skipNL();
        if (this.match("OP", ",")) { hadComma = true; if (this.check("OP", ")")) break; continue; }
        break;
      }
      this.expect("OP", ")");
      if (items.length === 1 && !hadComma && !named && !items[0].mut) return items[0].expr;   // 分组
      return { type: "TupleLit", named, items, loc: tl };
    }
    if (this.check("OP", "[")) {
      this.next();
      const items = [];
      this.skipNL();
      if (!this.check("OP", "]")) { do { this.skipNL(); items.push(this.parseExpr()); this.skipNL(); } while (this.match("OP", ",")); }
      this.expect("OP", "]", "']'");
      return { type: "ArrayLiteral", items };
    }
    if (tk.kind === "IDENT") {
      this.next();
      // 构造字面量 Account{ balance: initial }（match target 内不解析构造）
      if (this.check("OP", "{") && !this.noConstruct) {
        this.next();
        const fields = [];
        this.skipNL();
        if (!this.check("OP", "}")) {
          do {
            this.skipNL();
            const fname = this.expect("IDENT", undefined, "字段名").value;
            this.expect("OP", ":");
            fields.push({ name: fname, expr: this.parseExpr() });
            this.skipNL();
          } while (this.match("OP", ","));
        }
        this.expect("OP", "}", "'}'");
        return { type: "ConstructExpr", name: tk.value, fields, loc: { line: tk.line, col: tk.col } };
      }
      return { type: "Ident", name: tk.value, loc: { line: tk.line, col: tk.col } };
    }
    this.err("期望表达式，但遇到 '" + (tk.value || tk.kind) + "'", tk);
  }
}

function parse(src) { return new Parser(lex(src)).parseProgram(); }

module.exports = { parse, Parser };
