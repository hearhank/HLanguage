// H 语言编译后端（C 目标）——从 AST 生成 C 源码，经 zig cc / gcc 编译为原生二进制
// 纯块子集：struct/enum/match/函数/表达式/print
// "块 = 连续内存"在 C 中零运行时开销原样兑现
// 不支持的（class/并发/error/ref/move/数组）编译时拒绝

const { parse } = require("./parser");

function typeName(t) {
  if (t.type === "NamedType") return t.name;
  if (t.type === "ArrayType") return "[" + typeName(t.elem) + "]";
  if (t.type === "GenericType") return t.name;
  return "?";
}

function cType(tname) {
  switch (tname) {
    case "u64": return "unsigned long long";
    case "f64": return "double";
    case "bool": return "bool";
    case "Str": return "const char*";
    case "void": return "void";
    default: {
      // 动态块 [T] → 短名_Array（元素短名：u64/f64/Str/结构体名）
      if (tname.startsWith("[") && tname.endsWith("]")) {
        return shortName(tname.slice(1, -1)) + "_Array";
      }
      return tname;   // struct 类型名（typedef 已定义）
    }
  }
}
function shortName(tname) {
  // 元素类型短名（用于数组 typedef 名）：u64/f64/Str/bool/结构体名 本身即是短名
  return tname;
}

function genC(ast) {
  const enums = {}, rets = {}, structFields = {}, arrayElems = new Set();
  const collectTypes = (t) => {
    if (t.type === "ArrayType") { arrayElems.add(typeName(t.elem)); collectTypes(t.elem); }
  };
  // 从字面量推断数组元素类型（函数体内的 [10,20,30] 无类型注解）
  const literalElemType = (items) => {
    for (const it of items) {
      if (it.type === "Literal") {
        if (typeof it.value === "number") return Number.isInteger(it.value) ? "u64" : "f64";
        if (typeof it.value === "string") return "Str";
      }
      if (it.type === "ArrayLiteral") return "[" + literalElemType(it.items) + "]";
    }
    return "u64";
  };
  const scanExpr = (e) => {
    if (!e || typeof e !== "object") return;
    if (e.type === "ArrayLiteral") {
      arrayElems.add(literalElemType(e.items));
      e.items.forEach(scanExpr);
      return;
    }
    for (const v of Object.values(e)) {
      if (Array.isArray(v)) v.forEach(scanExpr);
      else if (v && typeof v === "object" && v.type) scanExpr(v);
    }
  };
  for (const d of ast.decls) {
    if (d.type === "EnumDecl") enums[d.name] = d.variants;
    if (d.type === "FunDecl") {
      rets[d.name] = d.ret ? typeName(d.ret.rtype) : "void";
      collectTypes(d.ret.rtype);
      for (const p of d.params) collectTypes(p.ptype);
      d.body.stmts.forEach(scanExpr);
    }
    if (d.type === "StructDecl") {
      structFields[d.name] = {};
      for (const f of d.fields) {
        structFields[d.name][f.name] = typeName(f.fieldType);
        collectTypes(f.fieldType);
      }
    }
  }
  const ctx = { enums, rets, structs: structFields };
  // 数组 typedef（动态块：连续数据区 + 长度）
  const arrayDefs = [...arrayElems].map(e =>
    `typedef struct { unsigned long long len; ${cType(e)}* data; } ${shortName(e)}_Array;`
  ).join("\n");
  const pre = [];
  const structs = [], enumsDefs = [], funcs = [];
  for (const d of ast.decls) {
    if (d.type === "StructDecl") structs.push(genStruct(d));
    else if (d.type === "EnumDecl") enumsDefs.push(genEnum(d, ctx));
    else if (d.type === "FunDecl") funcs.push(genFun(d, ctx));
    else if (d.type === "InterfaceDecl") { /* 契约，不生成 */ }
    else if (d.type === "ClassDecl") throw new Error("C 后端暂不支持 class（树）——请用 h run");
    else if (d.type === "GlobalDecl") throw new Error("C 后端暂不支持 global/并发——请用 h run");
    else if (d.type === "SpawnStmt" || d.type === "YieldStmt") throw new Error("C 后端暂不支持并发——请用 h run");
    else if (d.type === "ExprStmt") {
      const isMainCall = d.expr.type === "CallExpr" && d.expr.callee.type === "Ident" && d.expr.callee.name === "main";
      if (!isMainCall) throw new Error("C 后端暂不支持顶层语句——请用 h run");
    }
  }
  const body = [
    '#include <stdio.h>',
    '#include <stdbool.h>',
    '#include <stdint.h>',
    '',
    '/* 最短往返浮点格式化（与解释器 JS 输出一致） */',
    'static void h_print_f64(double d) {',
    '  char buf[64];',
    '  int prec;',
    '  for (prec = 17; prec >= 1; prec--) {',
    '    snprintf(buf, sizeof buf, "%.*g", prec, d);',
    '    double back;',
    '    if (sscanf(buf, "%lf", &back) == 1 && back == d) break;',
    '  }',
    '  printf("%s", buf);',
    '}',
    '',
  ];
  return body.concat(arrayDefs ? [arrayDefs, ""] : [], structs, enumsDefs, funcs, ["int main(void) {", "  h_main();", "  return 0;", "}", ""]).join("\n");
}

function genStruct(d) {
  const fields = d.fields.map(f => `  ${cType(typeName(f.fieldType))} ${f.name};`).join("\n");
  return `typedef struct {\n${fields}\n} ${d.name};`;
}
function genEnum(d) {
  const names = d.variants.map(v => `  ${d.name}_${v}`).join(",\n");
  return `typedef enum {\n${names}\n} ${d.name};`;
}

function genFun(d, ctx) {
  const scope = new Scope(null, ctx);
  const params = d.params.map(p => {
    scope.declareType(p.name, typeName(p.ptype));
    return `${cType(typeName(p.ptype))} ${p.name}`;
  }).join(", ");
  const body = d.body.stmts.map(s => genStmt(s, scope)).join("\n");
  const ret = d.ret ? cType(typeName(d.ret.rtype)) : "void";
  const fname = d.name === "main" ? "h_main" : d.name;
  return `${ret} ${fname}(${params}) {\n${body}\n}`;
}

class Scope {
  constructor(parent, ctx) { this.parent = parent || null; this.ctx = ctx; this.vars = new Set(); this.types = new Map(); }
  declared(name) { let s = this; while (s) { if (s.vars.has(name)) return true; s = s.parent; } return false; }
  declareType(name, t) { this.vars.add(name); this.types.set(name, t); }
  typeOf(name) { let s = this; while (s) { if (s.types.has(name)) return s.types.get(name); s = s.parent; } return null; }
}

function genStmt(st, scope) {
  switch (st.type) {
    case "VarDecl": {
      if (st.kind === "ref" || st.kind === "move") throw new Error("C 后端暂不支持 ref/move——请用 h run");
      const init = genExpr(st.init, scope);
      if (scope.declared(st.name)) return `  ${st.name} = ${init};`;
      const t = cType(inferType(st.init, scope) || "void");
      scope.declareType(st.name, inferType(st.init, scope));
      return `  ${t} ${st.name} = ${init};`;
    }
    case "ReturnStmt":
      return "  return " + (st.expr ? genExpr(st.expr, scope) : "") + ";";
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
    default:
      throw new Error("C 后端暂不支持语句 " + st.type);
  }
}

function genExpr(e, scope) {
  switch (e.type) {
    case "Literal": return cLiteral(e);
    case "Ident": return e.name;
    case "MemberExpr": {
      // 枚举值 Type.Variant → 常量名
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name + "_" + e.prop;
      // 数组 .len → .len 字段
      const ot = inferType(e.obj, scope);
      if (ot && ot.startsWith("[") && e.prop === "len") return genExpr(e.obj, scope) + ".len";
      return genExpr(e.obj, scope) + "." + e.prop;
    }
    case "CallExpr": return genCall(e, scope);
    case "BinExpr": {
      const l = genExpr(e.left, scope), r = genExpr(e.right, scope);
      return `(${l} ${e.op} ${r})`;
    }
    case "UnaryExpr": return `(${e.op}${genExpr(e.operand, scope)})`;
    case "AssignExpr": {
      const target = e.left.type === "Ident" ? e.left.name : genExpr(e.left, scope);
      return `${target} ${e.op === "=" ? "=" : e.op} ${genExpr(e.right, scope)}`;
    }
    case "ConstructExpr": {
      const fields = e.fields.map(f => `.${f.name} = ${genExpr(f.expr, scope)}`).join(", ");
      return `(${e.name}){ ${fields} }`;
    }
    case "MatchExpr": return genMatch(e, scope);
    case "ErrorLit": throw new Error("C 后端暂不支持 error——请用 h run");
    case "MoveExpr": throw new Error("C 后端暂不支持 move——请用 h run");
    case "ArrayLiteral": {
      const t = inferType(e, scope);   // "[f64]" 等
      const items = e.items.map(x => genExpr(x, scope)).join(", ");
      const et = cType(t.slice(1, -1));
      return `(${cType(t)}){ .len = ${e.items.length}, .data = (${et}[]){ ${items} } }`;
    }
    case "IndexExpr":
      return genExpr(e.obj, scope) + ".data[" + genExpr(e.index, scope) + "]";
    default: throw new Error("C 后端暂不支持表达式 " + e.type);
  }
}

function cLiteral(e) {
  if (e.kind === "string") return '"' + e.value.replace(/\\/g, "\\\\").replace(/"/g, '\\"') + '"';
  if (e.kind === "bool") return e.value ? "true" : "false";
  if (typeof e.value === "number") {
    if (Number.isInteger(e.value)) return e.value + "ULL";
    return e.value + "";
  }
  return String(e.value);
}

function genMatch(e, scope) {
  const enumName = inferEnumName(e.target, scope);
  const target = genExpr(e.target, scope);
  const retT = cType(inferType(e, scope) || "double");
  const cases = e.arms.map(arm => {
    const constName = enumName ? enumName + "_" + arm.variant : arm.variant;
    return `      case ${constName}: _h_m = ${genExpr(arm.expr, scope)}; break;`;
  }).join("\n");
  // GNU 语句表达式：switch 赋值临时变量，尾部取值为表达式结果（不能用 return——会返回外层函数）
  return `({ __typeof__(${target}) _h_t = (${target}); ${retT} _h_m = 0; switch (_h_t) {\n${cases}\n    default: break; } _h_m; })`;
}

function genCall(e, scope) {
  const callee = e.callee;
  const args = e.args;
  if (callee.type === "Ident") {
    if (callee.name === "print") return genPrint(args, scope);
    if (callee.name === "store" || callee.name === "load") throw new Error("C 后端暂不支持 store/load——请用 h run");
    const fn = scope.ctx.rets[callee.name];
    return `${callee.name === "main" ? "h_main" : callee.name}(${args.map(a => genExpr(a, scope)).join(", ")})`;
  }
  throw new Error("C 后端暂不支持该调用");
}

function genPrint(args, scope) {
  const parts = [];
  for (const a of args) {
    if (parts.length) parts.push('printf(" ");');
    const t = inferType(a, scope);
    const code = genExpr(a, scope);
    if (t === "f64") parts.push(`h_print_f64(${code});`);
    else if (t === "u64") parts.push(`printf("%llu", ${code});`);
    else if (t === "bool") parts.push(`printf("%s", (${code}) ? "true" : "false");`);
    else parts.push(`printf("%s", ${code});`);
  }
  parts.push('printf("\\n");');
  return parts.join("\n  ");
}

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
      if (typeof e.value === "number") return Number.isInteger(e.value) ? "u64" : "f64";
      if (typeof e.value === "string") return "Str";
      return "bool";
    case "Ident": return scope.typeOf(e.name) || "?";
    case "ConstructExpr": return e.name;
    case "MemberExpr": {
      // 枚举值 Type.Variant → 枚举名
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name;
      // 数组 .len → 数组类型
      const ot = inferType(e.obj, scope);
      if (ot && ot.startsWith("[") && e.prop === "len") return "u64";
      // struct 字段类型推断
      if (scope.ctx.structs && scope.ctx.structs[ot] && scope.ctx.structs[ot][e.prop]) {
        return scope.ctx.structs[ot][e.prop];
      }
      return "?";
    }
    case "ArrayLiteral": {
      const et = e.items.length ? inferType(e.items[0], scope) : "u64";
      return "[" + et + "]";
    }
    case "IndexExpr": {
      const ot = inferType(e.obj, scope);
      return ot && ot.startsWith("[") ? ot.slice(1, -1) : "?";
    }
    case "MatchExpr": return e.arms.length ? inferType(e.arms[0].expr, scope) : "?";
    case "BinExpr": {
      const l = inferType(e.left, scope), r = inferType(e.right, scope);
      if (l === "f64" || r === "f64") return "f64";
      if (l !== "?") return l;
      return r;
    }
    case "UnaryExpr": return inferType(e.operand, scope);
    case "CallExpr": {
      if (e.callee.type === "Ident" && scope.ctx.rets[e.callee.name]) return scope.ctx.rets[e.callee.name];
      return "?";
    }
    default: return "?";
  }
}

module.exports = { genC };
