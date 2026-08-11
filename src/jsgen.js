// H 语言编译后端（JS 目标）——从 AST 生成独立可执行的 JS 代码
// 当前切片：纯块子集（struct/enum/match/函数/表达式）——双后端一致性验证
// 不支持的（class/并发/error/ref/move）编译时拒绝，提示用 h run

const { parse } = require("./parser");

function typeName(t) {
  if (t.type === "NamedType") return t.name;
  if (t.type === "ArrayType") return "[]";
  if (t.type === "GenericType") return t.name;
  return "?";
}

function jsgen(ast) {
  const enums = {}, rets = {};
  for (const d of ast.decls) {
    if (d.type === "EnumDecl") enums[d.name] = d.variants;
    if (d.type === "FunDecl") rets[d.name] = d.ret ? typeName(d.ret.rtype) : "void";
  }
  const ctx = { enums, rets };
  const out = [];
  for (const d of ast.decls) {
    if (d.type === "StructDecl") out.push(genStruct(d));
    else if (d.type === "EnumDecl") out.push(genEnum(d));
    else if (d.type === "FunDecl") out.push(genFun(d, ctx));
    else if (d.type === "InterfaceDecl") { /* 编译期契约，不生成 */ }
    else if (d.type === "ClassDecl") throw new Error("编译后端暂不支持 class（树）——请用 h run");
    else if (d.type === "GlobalDecl") throw new Error("编译后端暂不支持 global/并发——请用 h run");
    else if (d.type === "SpawnStmt") throw new Error("编译后端暂不支持 spawn——请用 h run");
    else if (d.type === "ExprStmt") {
      // 顶层 main() 显式调用 → 跳过（自动生成入口）；其他顶层语句不支持
      const isMainCall = d.expr.type === "CallExpr" && d.expr.callee.type === "Ident" && d.expr.callee.name === "main";
      if (!isMainCall) throw new Error("编译后端暂不支持顶层语句——请用 h run");
    } else if (d.type === "YieldStmt") throw new Error("编译后端暂不支持顶层语句——请用 h run");
  }
  out.push("main();");
  return out.join("\n\n") + "\n";
}

function genStruct(d) {
  const lines = d.fields.map(f => `    this[${JSON.stringify(f.name)}] = fields && fields[${JSON.stringify(f.name)}] !== undefined ? fields[${JSON.stringify(f.name)}] : ${jsDefault(f.fieldType)};`);
  return `class ${d.name} {\n  constructor(fields) {\n${lines.join("\n")}\n  }\n}`;
}
function jsDefault(t) {
  if (t.type === "ArrayType") return "[]";
  if (t.type === "NamedType") {
    if (["u64", "f64"].includes(t.name)) return "0";
    if (t.name === "Str") return '""';
    if (t.name === "bool") return "false";
  }
  return "null";
}

function genEnum(d) {
  return d.variants.map(v => `const ${d.name}_${v} = ${JSON.stringify(d.name + "." + v)};`).join("\n");
}

/* ---------- 函数 ---------- */
function genFun(d, ctx) {
  const scope = new Scope(null, ctx);
  const params = d.params.map(p => {
    scope.declareType(p.name, typeName(p.ptype));
    return p.name;
  }).join(", ");
  const body = d.body.stmts.map(s => genStmt(s, scope)).join("\n");
  return `function ${d.name}(${params}) {\n${body}\n}`;
}

class Scope {
  constructor(parent, ctx) { this.parent = parent || null; this.ctx = ctx; this.vars = new Set(); this.types = new Map(); }
  declared(name) { let s = this; while (s) { if (s.vars.has(name)) return true; s = s.parent; } return false; }
  declare(name) { this.vars.add(name); }
  declareType(name, t) { this.vars.add(name); this.types.set(name, t); }
  typeOf(name) { let s = this; while (s) { if (s.types.has(name)) return s.types.get(name); s = s.parent; } return null; }
}

/* ---------- 语句 ---------- */
function genStmt(st, scope) {
  switch (st.type) {
    case "VarDecl": {
      if (st.kind === "ref") throw new Error("编译后端暂不支持 ref——请用 h run");
      const init = genExpr(st.init, scope);
      const t = inferType(st.init, scope);
      if (scope.declared(st.name)) return `  ${st.name} = ${init};`;
      scope.declareType(st.name, t);
      return `  let ${st.name} = ${init};`;
    }
    case "ReturnStmt":
      return "  return " + (st.expr ? genExpr(st.expr, scope) : "undefined") + ";";
    case "IfStmt": {
      const c = genExpr(st.cond, scope);
      const t = st.then.stmts.map(s => genStmt(s, scope)).join("\n");
      if (st.els) {
        const e = st.els.type === "Block" ? st.els.stmts.map(s => genStmt(s, scope)).join("\n") : genStmt(st.els, scope);
        return `  if (${c}) {\n${t}\n  } else {\n${e}\n  }`;
      }
      return `  if (${c}) {\n${t}\n  }`;
    }
    case "Block": {
      const inner = new Scope(scope, scope.ctx);
      return st.stmts.map(s => genStmt(s, inner)).join("\n");
    }
    case "ExprStmt":
      return "  " + genExpr(st.expr, scope) + ";";
    case "YieldStmt":
      throw new Error("编译后端暂不支持 yield——请用 h run");
    case "SpawnStmt":
      throw new Error("编译后端暂不支持 spawn——请用 h run");
    default:
      throw new Error("编译后端暂不支持语句 " + st.type);
  }
}

/* ---------- 表达式 ---------- */
function genExpr(e, scope) {
  switch (e.type) {
    case "Literal": return JSON.stringify(e.value);
    case "Ident": return e.name;
    case "MemberExpr": {
      // 枚举值 Type.Variant → 常量名；否则字段访问 obj.prop
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name + "_" + e.prop;
      return genExpr(e.obj, scope) + "." + e.prop;
    }
    case "CallExpr": return genCall(e, scope);
    case "BinExpr": {
      const l = genExpr(e.left, scope), r = genExpr(e.right, scope);
      const op = e.op === "==" ? "===" : e.op === "!=" ? "!==" : e.op;
      return `(${l} ${op} ${r})`;
    }
    case "UnaryExpr": return `(${e.op}${genExpr(e.operand, scope)})`;
    case "AssignExpr": {
      const target = e.left.type === "Ident" ? e.left.name : genExpr(e.left, scope);
      return `${target} ${e.op === "=" ? "=" : e.op} ${genExpr(e.right, scope)}`;
    }
    case "ConstructExpr": {
      const fields = e.fields.map(f => `${JSON.stringify(f.name)}: ${genExpr(f.expr, scope)}`).join(", ");
      return `new ${e.name}({ ${fields} })`;
    }
    case "MatchExpr": return genMatch(e, scope);
    case "ArrayLiteral":
      return "[" + e.items.map(x => genExpr(x, scope)).join(", ") + "]";
    case "IndexExpr":
      return genExpr(e.obj, scope) + "[" + genExpr(e.index, scope) + "]";
    case "ErrorLit":
      throw new Error("编译后端暂不支持 error——请用 h run");
    case "MoveExpr":
      throw new Error("编译后端暂不支持 move——请用 h run");
    default:
      throw new Error("编译后端暂不支持表达式 " + e.type);
  }
}

function genMatch(e, scope) {
  const enumName = inferEnumName(e.target, scope);
  const target = genExpr(e.target, scope);
  const cases = e.arms.map(arm => {
    const constName = enumName ? enumName + "_" + arm.variant : arm.variant;
    const val = genExpr(arm.expr, scope);
    return `      case ${constName}:\n        return ${val};`;
  }).join("\n");
  return `(() => {\n  switch (${target}) {\n${cases}\n    default:\n      throw new Error("match 未穷尽");\n  }\n})()`;
}

function genCall(e, scope) {
  const callee = e.callee;
  const args = e.args.map(a => genExpr(a, scope));
  if (callee.type === "Ident") {
    if (callee.name === "print") return `console.log([${args.join(", ")}].map(String).join(" "))`;
    if (callee.name === "store" || callee.name === "load") throw new Error("编译后端暂不支持 store/load——请用 h run");
    return `${callee.name}(${args.join(", ")})`;
  }
  if (callee.type === "MemberExpr") {
    const m = callee.prop;
    if (m === "to_str") return `String(${genExpr(callee.obj, scope)})`;
    if (m === "to_bytes" || m === "from_bytes" || m === "len") throw new Error("编译后端暂不支持字节方法——请用 h run");
    throw new Error("编译后端暂不支持方法调用 " + m);
  }
  throw new Error("无法编译该调用");
}

/* ---------- 类型推断（编译期符号表） ---------- */
function inferEnumName(e, scope) {
  if (e.type === "Ident") {
    const t = scope.typeOf(e.name);
    return t && t !== "?" && t !== "void" ? t : null;
  }
  if (e.type === "MemberExpr" && e.obj.type === "Ident") return e.obj.name;
  return null;
}
function inferType(e, scope) {
  switch (e.type) {
    case "Literal":
      if (typeof e.value === "number") return String(e.value).includes(".") ? "f64" : "u64";
      if (typeof e.value === "string") return "Str";
      return "bool";
    case "Ident": return scope.typeOf(e.name) || "?";
    case "ConstructExpr": return e.name;
    case "MemberExpr":
      if (e.obj.type === "Ident") return e.obj.name;   // 枚举值 → 枚举名
      return "?";
    case "MatchExpr": {
      const t = inferType(e.target, scope);
      return e.arms.length ? inferType(e.arms[0].expr, scope) : "?";
    }
    case "BinExpr": {
      const l = inferType(e.left, scope), r = inferType(e.right, scope);
      if (l === "f64" || r === "f64") return "f64";
      if (l !== "?" ) return l;
      return r;
    }
    case "UnaryExpr": return inferType(e.operand, scope);
    case "CallExpr": {
      if (e.callee.type === "Ident" && scope.ctx.rets[e.callee.name]) return scope.ctx.rets[e.callee.name];
      return "?";
    }
    case "ArrayLiteral": return "[]";
    default: return "?";
  }
}

module.exports = { jsgen };
