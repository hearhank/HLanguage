// H 语言语义检查器（从原型 #4 提取并修复）
// 静态规则 R1-R9：块只含块 / class 字段分型 / 写指针需可写源 / 只读不能写 /
// 类型存在性 / 接口实现 / 变量已定义 / 全局需模式 / move 后失效

const { parse } = require("./parser");

const BUILTIN_BLOCK = ["u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "Str", "bool", "void"];
const PATTERN_TYPES = ["Exclusive", "SharedRead", "Channel"];
const BUILTIN_METHODS = ["to_bytes", "from_bytes", "to_str", "to_string", "len", "push", "pop", "send", "recv", "alloc", "free", "clone"];
const BUILTIN_FUNCS = ["store", "load", "transmit"];
/* 数值类型：整数除法与提升判断 */
const NUM_TYPES = new Set(["u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64"]);
const numRank = (n) => ({ "u8": 1, "i8": 2, "u16": 3, "i16": 4, "u32": 5, "i32": 6, "u64": 7, "i64": 8, "u128": 9, "i128": 10, "usize": 7, "isize": 8, "f32": 20, "f64": 21 }[n] || 0);
const promoteNum = (a, b) => {
  if (a === "f64" || b === "f64") return "f64";
  if (a === "f32" || b === "f32") return "f32";
  const ra = numRank(a), rb = numRank(b);
  if (!ra && !rb) return null;
  return ra >= rb ? a : b;
};

class Scope {
  constructor(parent) { this.parent = parent; this.vars = {}; }
  define(name, info) { this.vars[name] = info; }
  lookup(name) { return this.vars[name] || (this.parent && this.parent.lookup(name)); }
}

class Checker {
  constructor(ast) {
    this.ast = ast;
    this.types = {};
    this.funcs = {};
    this.errors = [];
    this.globals = new Set();
    this.loopDepth = 0;
  }
  err(rule, msg, loc) { this.errors.push({ rule, msg, line: loc ? loc.line : 0, col: loc ? loc.col : 0 }); }

  register() {
    for (const d of this.ast.decls) {
      if (d.type === "GlobalDecl") { this.globals.add(d.name); continue; }
      if (d.type === "StructDecl") {
        const fields = {};
        for (const f of d.fields) fields[f.name] = { fieldType: f.fieldType, isMut: f.isMut };
        this.types[d.name] = { shape: "block", kind: "struct", fields, methods: [], interfaces: [], loc: d.loc };
      } else if (d.type === "ClassDecl") {
        const fields = {};
        for (const f of d.fields) fields[f.name] = { fieldType: f.fieldType, isMut: f.isMut };
        this.types[d.name] = { shape: "tree", kind: "class", fields, methods: d.methods, interfaces: d.interfaces, imports: d.imports, hides: d.hides, aliases: d.aliases, loc: d.loc };
      } else if (d.type === "EnumDecl") {
        this.types[d.name] = { shape: "block", kind: "enum", fields: {}, methods: [], interfaces: [], variants: d.variants, loc: d.loc };
      } else if (d.type === "InterfaceDecl") {
        this.types[d.name] = { shape: "iface", kind: "interface", fields: {}, methods: d.methods, interfaces: [], loc: d.loc };
      } else if (d.type === "FunDecl") {
        this.funcs[d.name] = d;
      }
    }
  }

  shapeOf(t) {
    if (t.type === "ArrayType") {
      const es = this.shapeOf(t.elem);
      if (es !== "block" && es !== "unknown") this.err("R1", "动态块元素必须是块类型（连续内存），但 " + t.elem.name + " 是" + (es === "tree" ? "树" : "未知") + "类型", null);
      return "block";
    }
    if (t.type === "SliceType") {
      const es = this.shapeOf(t.elem);
      if (es !== "block" && es !== "unknown") this.err("R1", "切片元素必须是块类型，但 " + (t.elem.name || "?") + " 是" + (es === "tree" ? "树" : "未知") + "类型", null);
      return "slice";   // 借用视图：块数据的引用（生命周期受限，R12）
    }
    if (t.type === "TupleType") {
      for (const it of t.items) {
        const es = this.shapeOf(it.type);
        if (es !== "block" && es !== "unknown") this.err("R1", "元组元素必须是块类型（连续内存），但 " + (it.name || it.type.name || "?") + " 是" + (es === "tree" ? "树" : "未知") + "类型", null);
      }
      return "block";
    }
    if (t.type === "OptionalType") {
      const es = this.shapeOf(t.inner);
      if (es !== "block" && es !== "unknown") this.err("R1", "可选值必须是块类型（连续内存），但 " + (t.inner.name || "?") + " 是" + (es === "tree" ? "树" : "未知") + "类型", null);
      return "block";
    }
    if (t.type === "FunType") return "block";   // 函数引用（无捕获）= 纯代码引用，块值
    if (t.type === "GenericType") return "block";
    const def = this.types[t.name];
    if (!def) {
      if (BUILTIN_BLOCK.includes(t.name)) return "block";   // 内建块类型（u64/f64/Str/bool/void）
      this.err("R5", "未定义的类型 '" + t.name + "'", null);
      return "unknown";
    }
    return def.shape;
  }
  nameOf(t) {
    return t.type === "NamedType" ? t.name
      : t.type === "ArrayType" ? "[" + (t.elem.name || "?") + "]"
      : t.type === "SliceType" ? "[]" + (t.elem.name || "?")
      : t.type === "TupleType" ? (t.named
          ? "(" + t.items.map(i => i.name + ": " + (i.type.name || "?")).join(", ") + ")"
          : "(" + t.items.map(i => i.type.name || "?").join(", ") + ")")
      : t.type === "OptionalType" ? "?" + this.nameOf(t.inner)
      : t.type === "FunType" ? "fun(" + t.params.map(p => this.nameOf(p)).join(", ") + ") -> " + (t.ret ? this.nameOf(t.ret) : "void")
      : t.type === "GenericType" ? t.name + "<>"
      : "?";
  }

  checkTypeShapes() {
    for (const d of this.ast.decls) {
      if (d.type === "StructDecl") {
        for (const f of d.fields) {
          if (f.fieldType.mutable) this.err("R1", "块（struct）不能有 ref 字段：'" + f.name + "'", f.loc);
          const sh = this.shapeOf(f.fieldType);
          if (sh === "slice") this.err("R12", "切片不得存入字段（借入不借出）：'" + f.name + "'", f.loc);
          if (sh === "tree") this.err("R1", "块只含块：字段 '" + f.name + "' 引用了树类型 " + (f.fieldType.name || "?") + "（树有生命周期，块是纯数据）", f.loc);
        }
      } else if (d.type === "ClassDecl") {
        for (const f of d.fields) {
          const sh = this.shapeOf(f.fieldType);
          if (sh === "slice") this.err("R12", "切片不得存入字段（借入不借出）：'" + f.name + "'", f.loc);
          if (f.fieldType.mutable) {
            if (sh !== "tree") this.err("R2", "引用字段（ref）必须指向树类型，但 '" + f.name + "' 是 " + (this.nameOf(f.fieldType) || "未知") + "（" + (sh === "block" ? "块" : "未知") + "）", f.loc);
          } else {
            if (sh === "tree") this.err("R2", "class 的值字段必须是块，但 '" + f.name + "' 是树类型 " + (f.fieldType.name || "?") + "（树只能通过 ref 引用字段连接）", f.loc);
          }
        }
      }
    }
  }

  /* import 语义：方法提升（深度传递、循环拒绝 R11）、同名冲突强制处理、接口继承 */
  computeMethods() {
    this.methods = {};          // 类名 -> {方法名: {func, source}}
    this.effInterfaces = {};    // 类名 -> [接口名]（含导入继承，减 hide）
    const resolve = (clsName, visiting) => {
      if (this.methods[clsName]) return { table: this.methods[clsName], ifaces: this.effInterfaces[clsName] };
      if (visiting.has(clsName)) {
        this.err("R11", "class 导入循环：'" + clsName + "'", this.types[clsName] ? this.types[clsName].loc : null);
        return { table: {}, ifaces: [] };
      }
      visiting.add(clsName);
      const cls = this.types[clsName];
      if (!cls || cls.kind !== "class") {
        this.err("R11", "导入/别名引用的类 '" + clsName + "' 不存在", null);
        visiting.delete(clsName);
        return { table: {}, ifaces: [] };
      }
      const table = {};
      for (const m of cls.methods) table[m.name] = { func: m, source: clsName };
      const ifaces = cls.interfaces.slice();
      for (const imp of cls.imports) {
        const sub = resolve(imp.name, visiting);
        for (const [n, entry] of Object.entries(sub.table)) {
          if (!table[n]) { table[n] = entry; continue; }
          // 同名冲突：自己的优先（自动隐藏导入）；导入之间必须显式处理
          if (table[n].source !== entry.source && table[n].source !== clsName) {
            const handled = cls.hides.some(h => h.path.parts.length >= 2 && h.path.parts[1] === n)
              || cls.aliases.some(al => al.path.parts[1] === n && al.alias !== n);
            if (!handled) this.err("R11", "class " + clsName + " 导入的 '" + n + "' 存在同名冲突，必须 hide 或 alias 处理", cls.loc);
          }
        }
        for (const i of sub.ifaces) if (!ifaces.includes(i)) ifaces.push(i);
      }
      for (const h of cls.hides) {
        if (h.path.parts.length >= 2) {
          const [src, mname] = h.path.parts;
          if (table[mname] && table[mname].source === src) delete table[mname];
        } else {
          const idx = ifaces.indexOf(h.path.parts[0]);
          if (idx >= 0) ifaces.splice(idx, 1);
        }
      }
      for (const al of cls.aliases) {
        const [src, mname] = al.path.parts;
        const srcTable = this.methods[src] || resolve(src, visiting).table;
        if (srcTable[mname]) table[al.alias] = srcTable[mname];
        else this.err("R11", "alias '" + al.alias + "' 引用的 " + src + "::" + mname + " 不存在", al.loc || cls.loc);
      }
      visiting.delete(clsName);
      this.methods[clsName] = table;
      this.effInterfaces[clsName] = ifaces;
      return { table, ifaces };
    };
    for (const n of Object.keys(this.types)) {
      if (this.types[n].kind === "class") resolve(n, new Set());
    }
  }

  checkInterfaces() {
    for (const d of this.ast.decls) {
      if (d.type !== "ClassDecl") continue;
      const ifaces = this.effInterfaces[d.name] || d.interfaces;
      const table = this.methods[d.name] || {};
      for (const iname of ifaces) {
        const iface = this.types[iname];
        if (!iface) { this.err("R5", "接口 '" + iname + "' 未定义", d.loc); continue; }
        for (const sig of iface.methods) {
          const m = table[sig.name] && table[sig.name].func;
          if (!m) { this.err("R6", "class " + d.name + " 声明实现接口 " + iname + "，但缺少方法 '" + sig.name + "'", d.loc); continue; }
          if (m.params.length !== sig.params.length) {
            this.err("R6", "方法 '" + sig.name + "' 的参数数量与接口签名不符（期望 " + sig.params.length + "，实际 " + m.params.length + "）", m.loc);
          }
          const sr = sig.ret, mr = m.ret;
          if (sr && mr && this.shapeOf(sr.rtype) !== "unknown" && this.shapeOf(mr.rtype) !== this.shapeOf(sr.rtype)) {
            this.err("R6", "方法 '" + sig.name + "' 返回类型不匹配接口（期望 " + this.nameOf(sr.rtype) + "）", m.loc);
          }
        }
      }
    }
  }

  checkGlobals() {
    for (const d of this.ast.decls) {
      if (d.type !== "GlobalDecl") continue;
      if (this.shapeOf(d.gtype) === "slice") this.err("R12", "切片不得存入全局（借入不借出）：'" + d.name + "'", d.loc);
      const gt = d.gtype;
      if (gt.type !== "GenericType" || !PATTERN_TYPES.includes(gt.name)) {
        this.err("R8", "global 必须声明访问模式（Exclusive<T>/SharedRead<T>/Channel<T> 等内建包装类型）", d.loc);
      }
    }
  }

  checkFuncs() {
    for (const d of this.ast.decls) {
      if (d.type !== "FunDecl") continue;
      const scope = new Scope(null);
      for (const p of d.params) {
        const sh = this.shapeOf(p.ptype);
        scope.define(p.name, { shape: sh, name: this.nameOf(p.ptype), mutable: p.kind === "ref" || p.kind === "move", moved: false });
      }
      this.checkBlock(d.body, scope, d);
    }
  }

  checkBlock(block, scope, fun) {
    const local = new Scope(scope);
    for (const st of block.stmts) {
      switch (st.type) {
        case "VarDecl": this.checkVarDecl(st, local); break;
        case "ReturnStmt":
          if (st.expr) {
            const r = this.checkExpr(st.expr, local);
            if (fun && fun.ret) this.checkReturnShape(fun.ret, r, st);
          }
          break;
        case "IfStmt":
          this.checkExpr(st.cond, local);
          this.checkBlock(st.then, local, fun);
          if (st.els) this.checkBlock(st.els, local, fun);
          break;
        case "ForStmt": {
          // for i in 0..n：区间必须是数字 RangeExpr（切片区间不适用）
          if (st.range.type !== "RangeExpr") { this.err("R5", "for 的 in 必须是数字区间（0..n）", st.loc); break; }
          const ro = this.checkExpr(st.range.obj, local);
          if (ro.name === "f64") { this.err("R5", "for 区间要求整数（u64），不支持 f64", st.range.loc); break; }
          if (ro.name !== "u64") this.err("R5", "for 区间必须是数值（u64），但 '" + (ro.name || "?") + "' 不是", st.range.loc);
          if (st.range.start) this.checkExpr(st.range.start, local);
          if (st.range.end) this.checkExpr(st.range.end, local);
          const loop = new Scope(local);
          loop.define(st.varName, { shape: "block", name: "u64", mutable: false, moved: false });
          this.loopDepth++;
          this.checkBlock(st.body, loop, fun);
          this.loopDepth--;
          break;
        }
        case "WhileStmt":
          this.checkExpr(st.cond, local);
          this.loopDepth++;
          this.checkBlock(st.body, local, fun);
          this.loopDepth--;
          break;
        case "BreakStmt":
          if (!this.loopDepth) this.err("R5", "break 只能在循环内使用", st.loc);
          break;
        case "ContinueStmt":
          if (!this.loopDepth) this.err("R5", "continue 只能在循环内使用", st.loc);
          break;
        case "Block": this.checkBlock(st, local, fun); break;
        case "ExprStmt": this.checkExpr(st.expr, local); break;
        case "SpawnStmt": {
          this.checkExpr(st.callee, scope);
          // R12：切片不得作 spawn 参数（引用不跨执行体）
          if (st.callee.type === "CallExpr") {
            for (const a of st.callee.args) {
              const ar = this.checkExpr(a, scope);
              if (ar.shape === "slice") this.err("R12", "切片不得作 spawn 参数（引用不跨执行体）", a.loc);
            }
          }
          break;
        }
        case "YieldStmt": break;
      }
    }
  }

  checkReturnShape(ret, r, st) {
    if (ret.kind === "error") {
      if (st.expr && st.expr.type === "ErrorLit") return;
      if (r.shape === "unknown") return;
      if (this.shapeOf(ret.rtype) === "unknown") return;
      if (r.shape !== this.shapeOf(ret.rtype)) this.err("R4", "返回值形状不匹配（期望 " + this.nameOf(ret.rtype) + "）", st.loc);
      return;
    }
    if (ret.kind === "move" || ret.kind === "ref") {
      if (this.shapeOf(ret.rtype) === "slice") this.err("R12", "切片不得作为返回值（借入不借出）", st.loc);
      return;
    }
    if (this.shapeOf(ret.rtype) === "slice") this.err("R12", "切片不得作为返回值（借入不借出）", st.loc);
    if (r.shape === "unknown" || this.shapeOf(ret.rtype) === "unknown") return;
    if (r.shape !== this.shapeOf(ret.rtype)) this.err("R4", "返回值形状不匹配（期望 " + this.nameOf(ret.rtype) + "，实际 " + r.name + "）", st.loc);
  }

  funTypeOf(fn) {
    return "fun(" + fn.params.map(p => this.nameOf(p.ptype)).join(", ") + ") -> " + (fn.ret && fn.ret.rtype ? this.nameOf(fn.ret.rtype) : "void");
  }

  checkVarDecl(v, scope) {
    if (!v.init) return;
    // 覆盖声明（同名变量已存在）= 赋值语义 → R4 只读检查
    const existing = scope.lookup(v.name);
    if (v.kind === "val" && existing) {
      if (!existing.mutable) this.err("R4", "赋值目标不可写（只读变量）：'" + v.name + "'", v.loc);
      this.checkExpr(v.init, scope);
      return;
    }
    const init = this.checkExpr(v.init, scope);
    if (v.annotation) this.shapeOf(v.annotation);
    if (v.kind === "ref") {
      if (!init.mutable) this.err("R3", "只能对可写变量/数据声明可写指针（ref）—— '" + (v.init.type === "Ident" ? v.init.name : "该表达式") + "' 不可写", v.loc);
    }
    scope.define(v.name, { shape: init.shape, name: v.annotation ? this.nameOf(v.annotation) : init.name, mutable: v.kind === "mut" || v.kind === "ref", moved: false });
  }

  checkExpr(e, scope) {
    if (!e) return { shape: "unknown", name: "?", mutable: false };
    switch (e.type) {
      case "Literal":
        return { shape: "block", name: e.kind === "string" ? "Str" : e.kind === "bool" ? "bool" : e.kind === "null" ? "?" : e.ltype || (e.kind === "float" ? "f64" : "u64"), mutable: false };
      case "Ident": {
        const v = scope.lookup(e.name);
        if (!v) {
          if (this.globals.has(e.name) || this.types[e.name]) return { shape: "unknown", name: e.name, mutable: true };  // global/类型名放行
          if (this.funcs[e.name]) return { shape: "block", name: this.funTypeOf(this.funcs[e.name]), mutable: false };   // 函数名 → 函数引用值
          this.err("R7", "未定义的变量 '" + e.name + "'", e.loc); return { shape: "unknown", name: e.name, mutable: false };
        }
        if (v.moved) this.err("R9", "变量 '" + e.name + "' 已被 move，不能再使用", e.loc);
        return { shape: v.shape, name: v.name, mutable: v.mutable };
      }
      case "MemberExpr": {
        if (e.obj.type === "Ident" && this.types[e.obj.name]) {
          const tdef = this.types[e.obj.name];
          if (tdef.kind === "enum") {
            if (!tdef.variants.includes(e.prop)) { this.err("R10", "枚举 " + e.obj.name + " 没有变体 '" + e.prop + "'", e.loc); return { shape: "unknown", name: "?", mutable: false }; }
            return { shape: "block", name: e.obj.name, mutable: false };
          }
          if (BUILTIN_METHODS.includes(e.prop)) return { shape: "unknown", name: "?", mutable: false };
          this.err("R5", "类型 '" + e.obj.name + "' 没有静态方法 '" + e.prop + "'", e.loc);
          return { shape: "unknown", name: "?", mutable: false };
        }
        const obj = this.checkExpr(e.obj, scope);
        if (BUILTIN_METHODS.includes(e.prop)) return { shape: "unknown", name: "?", mutable: false };
        if (obj.shape === "slice" && e.prop === "len") return { shape: "block", name: "u64", mutable: false };
        // 元组访问：命名 .x / 位置 .0（元组名格式 "(T1, T2)" / "(x: T1, y: T2)"）
        if (obj.name && obj.name.startsWith("(") && obj.name.endsWith(")")) {
          const inner = obj.name.slice(1, -1);
          const parts = inner === "" ? [] : inner.split(", ").map(p => p.includes(": ") ? { name: p.split(": ")[0], t: p.split(": ")[1] } : { name: null, t: p });
          if (parts.length && parts[0].name !== null) {
            const f = parts.find(p => p.name === e.prop);
            if (!f) { this.err("R5", "元组没有字段 '" + e.prop + "'", e.loc); return { shape: "unknown", name: "?", mutable: false }; }
            return { shape: "block", name: f.t, mutable: false };
          }
          const idx = Number(e.prop);
          if (!Number.isInteger(idx) || idx < 0 || idx >= parts.length) { this.err("R5", "元组索引越界 '" + e.prop + "'（共 " + parts.length + " 个元素）", e.loc); return { shape: "unknown", name: "?", mutable: false }; }
          return { shape: "block", name: parts[idx].t, mutable: false };
        }
        const def = obj.shape === "unknown" ? null : this.types[obj.name];
        if (obj.shape === "unknown") return { shape: "unknown", name: "?", mutable: false };
        if (!def || !def.fields) {
          if (def && def.kind === "enum") this.err("R5", "枚举 " + def.name + " 没有字段 '" + e.prop + "'", e.loc);
          return { shape: "unknown", name: "?", mutable: false };
        }
        const f = def.fields[e.prop];
        if (!f) { this.err("R5", "类型 " + obj.name + " 没有字段 '" + e.prop + "'", e.loc); return { shape: "unknown", name: "?", mutable: false }; }
        return { shape: this.shapeOf(f.fieldType), name: this.nameOf(f.fieldType), mutable: obj.mutable && f.isMut };
      }
      case "CallExpr": {
        // 参数必须检查（类型标注/未定义变量/可写性）——既有缺口：此前 print 等内建参数从不检查
        for (const a of e.args) this.checkExpr(a, scope);
        const callee = e.callee;
        if (callee.type === "Ident") {
          const fn = this.funcs[callee.name];
          if (fn) {
            // R3：ref 参数实参必须是可写变量（写透别名需要可写源；否则运行时退化，禁止）
            fn.params.forEach((p, i) => {
              if (p.kind !== "ref") return;
              const arg = e.args[i];
              if (!arg || arg.type !== "Ident") { this.err("R3", "ref 参数 '" + p.name + "' 的实参必须是可写变量（不能是表达式）", e.loc); return; }
              const av = scope.lookup(arg.name);
              if (!av || !av.mutable) this.err("R3", "ref 参数 '" + p.name + "' 的实参 '" + arg.name + "' 不可写（需要 mut 变量）", e.loc);
            });
            if (fn.ret) return { shape: this.shapeOf(fn.ret.rtype), name: this.nameOf(fn.ret.rtype), mutable: false };
          }
          if (BUILTIN_FUNCS.includes(callee.name)) return { shape: "unknown", name: "?", mutable: false };
          return { shape: "unknown", name: "?", mutable: false };
        }
        if (callee.type === "MemberExpr") {
          // 静态调用（Type.from_bytes 等）：类型名放行，不查变量
          if (callee.obj.type === "Ident" && this.types[callee.obj.name]) return { shape: "unknown", name: "?", mutable: false };
          const obj = this.checkExpr(callee.obj, scope);
          const table = obj.shape === "unknown" ? null : this.methods[obj.name];
          const entry = table && table[callee.prop];
          if (entry && entry.func) {
            entry.func.params.forEach((p, i) => {
              if (p.kind !== "ref") return;
              const arg = e.args[i];
              if (!arg || arg.type !== "Ident") { this.err("R3", "ref 参数 '" + p.name + "' 的实参必须是可写变量（不能是表达式）", e.loc); return; }
              const av = scope.lookup(arg.name);
              if (!av || !av.mutable) this.err("R3", "ref 参数 '" + p.name + "' 的实参 '" + arg.name + "' 不可写（需要 mut 变量）", e.loc);
            });
          }
        }
        return { shape: "unknown", name: "?", mutable: false };
      }
      case "BinExpr": {
        const l = this.checkExpr(e.left, scope);
        const r = this.checkExpr(e.right, scope);
        // 可选值不能直接参与运算（需先解包 x.?）；与 null 的比较是允许的（裸 "?" 是未知类型，不误伤）
        const lt = l.name && l.name.startsWith("?") && l.name !== "?";
        const rt = r.name && r.name.startsWith("?") && r.name !== "?";
        if ((lt || rt) && !["==", "!=", "&&", "||"].includes(e.op)) this.err("R5", "可选值不能直接参与运算，需先解包（x.?）", e.loc);
        // 数值类型推断并标注（求值器据此决定整数整除 vs 浮点除法；f32 单精度截断）
        const ltn = NUM_TYPES.has(l.name) ? l.name : null;
        const rtn = NUM_TYPES.has(r.name) ? r.name : null;
        e._t = promoteNum(ltn, rtn);
        const isStr = l.name === "Str" || r.name === "Str";
        return { shape: "block", name: e._t || (isStr ? "Str" : "u64"), mutable: false };
      }
      case "UnwrapExpr": {
        const inner = this.checkExpr(e.expr, scope);
        const n = inner.name || "";
        if (n.startsWith("?")) return { shape: "block", name: n.slice(1), mutable: inner.mutable };
        if (n === "?" || n === "unknown") return { shape: "block", name: "?", mutable: false };
        this.err("R5", "解包目标必须是可选类型（?T），但 '" + (n || "?") + "' 不是", e.loc);
        return { shape: "block", name: "?", mutable: false };
      }
      case "UnaryExpr": { const r = this.checkExpr(e.operand, scope); return { shape: r.shape, name: r.name, mutable: false }; }
      case "MoveExpr": {
        const inner = this.checkExpr(e.expr, scope);
        if (e.expr.type === "Ident") {
          const v = scope.lookup(e.expr.name);
          if (v) v.moved = true;
        }
        return { shape: inner.shape, name: inner.name, mutable: true };
      }
      case "TupleLit": {
        const ts = e.items.map(it => { const r = this.checkExpr(it.expr, scope); return r.name || "?"; });
        return { shape: "block", name: (e.named ? "(" + e.items.map((it, i) => it.name + ": " + ts[i]).join(", ") + ")" : "(" + ts.join(", ") + ")"), mutable: false };
      }
      case "RangeExpr": {
        const obj = this.checkExpr(e.obj, scope);
        if (e.start) this.checkExpr(e.start, scope);
        if (e.end) this.checkExpr(e.end, scope);
        const n = obj.name || "";
        if (!(n.startsWith("[") && n.endsWith("]")) && !n.startsWith("[]")) this.err("R5", "取区间目标必须是动态块或切片，但 '" + (obj.name || "?") + "' 不是", e.loc);
        const elem = n.startsWith("[]") ? n.slice(2) : n.slice(1, -1);
        return { shape: "slice", name: "[]" + elem, mutable: false };
      }
      case "AssignExpr": {
        // 解构 (a, b) = expr：逐元素——新建变量或覆盖已有（可写检查 R4）
        if (e.left.type === "TupleLit") {
          for (const it of e.left.items) {
            if (!it.expr || it.expr.type !== "Ident") { this.err("R4", "解构目标必须是变量名", e.loc); continue; }
            const existing = scope.lookup(it.expr.name);
            if (existing) {
              if (!existing.mutable) this.err("R4", "解构目标不可写（只读变量）：'" + it.expr.name + "'", e.loc);
            } else {
              scope.define(it.expr.name, { shape: "block", name: "?", mutable: !!it.mut, moved: false });
            }
          }
          this.checkExpr(e.right, scope);
          return { shape: "unknown", name: "?", mutable: false };
        }
        const left = this.checkExpr(e.left, scope);
        const right = this.checkExpr(e.right, scope);
        if (!left.mutable) this.err("R4", "赋值目标不可写（只读变量 / 只读指针 / 非 mut 字段）", e.loc);
        else if (left.shape !== "unknown" && right.shape !== "unknown" && left.shape !== right.shape) {
          this.err("R4", "赋值类型不匹配：" + left.name + " ← " + right.name, e.loc);
        }
        return { shape: left.shape, name: left.name, mutable: false };
      }
      case "ConstructExpr": {
        const def = this.types[e.name];
        if (!def) { this.err("R5", "未定义的类型 '" + e.name + "'", e.loc); return { shape: "unknown", name: e.name, mutable: true }; }
        for (const f of e.fields) {
          if (!def.fields[f.name]) this.err("R5", "类型 " + e.name + " 没有字段 '" + f.name + "'", e.loc);
          else this.checkExpr(f.expr, scope);
        }
        return { shape: def.shape, name: e.name, mutable: true };
      }
      case "ErrorLit":
        return { shape: "error", name: "error", mutable: false };
      case "MatchExpr": {
        const target = this.checkExpr(e.target, scope);
        const enumDef = target.name && this.types[target.name] && this.types[target.name].kind === "enum" ? this.types[target.name] : null;
        const covered = new Set();
        let result = { shape: "unknown", name: "?", mutable: false };
        e.arms.forEach((arm, i) => {
          if (enumDef && !enumDef.variants.includes(arm.variant)) this.err("R10", "枚举 " + target.name + " 没有变体 '" + arm.variant + "'", arm.loc);
          covered.add(arm.variant);
          const ar = this.checkExpr(arm.expr, scope);
          if (i === 0) result = ar;
        });
        if (enumDef) {
          const missing = enumDef.variants.filter(v => !covered.has(v));
          if (missing.length) this.err("R10", "match 未穷尽：缺少变体 " + missing.join(", "), e.loc);
        }
        if (!enumDef && target.shape !== "unknown") {
          this.err("R10", "match 目标必须是枚举，但 '" + target.name + "' 不是枚举", e.loc);
        }
        return result;
      }
      case "ArrayLiteral": {
        for (const it of e.items) this.checkExpr(it, scope);
        return { shape: "block", name: "[]", mutable: false };
      }
      case "IndexExpr": {
        const obj = this.checkExpr(e.obj, scope);
        this.checkExpr(e.index, scope);
        const n = obj.name || "";
        const elem = n.startsWith("[]") ? n.slice(2) : n.startsWith("[") ? n.slice(1, -1) : "?";
        return { shape: "block", name: elem, mutable: obj.mutable };   // 元素可写性继承自目标（数组/切片需 mut）
      }
      default: return { shape: "unknown", name: "?", mutable: false };
    }
  }
}

function check(src) {
  const ast = parse(src);
  const c = new Checker(ast);
  c.register();
  c.computeMethods();
  c.checkTypeShapes();
  c.checkInterfaces();
  c.checkGlobals();
  c.checkFuncs();
  return { ast, types: c.types, funcs: c.funcs, errors: c.errors };
}

module.exports = { check, Checker };
