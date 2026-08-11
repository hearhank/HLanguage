// H 语言编译后端（C 目标）——从 AST 生成 C 源码，经 zig cc / gcc 编译为原生二进制
// 支持：struct（块）/enum + match/动态数组 [T]/class（树：堆分配 + 生命周期 + move + 静态派发）
// 树在 C 中 = 指针（Type*）：构造 h_new_Type / 作用域退出 h_free_Type / 方法静态派发 Type_method(self,...)
// 不支持的（并发/error/ref 指针/ref 字段/顶层语句）编译时拒绝

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
      if (tname.startsWith("[") && tname.endsWith("]")) return shortName(tname.slice(1, -1)) + "_Array";
      return tname;   // struct/class 类型名（typedef 已定义）
    }
  }
}
function shortName(tname) { return tname; }

/* 标量/复合值的打印语句（双后端一致：对齐求值器 valueToStr） */
function scalarPrint(t, e, ctx) {
  if (t === "f64") return `h_print_f64(${e});`;
  if (t === "u64") return `printf("%llu", ${e});`;
  if (t === "bool") return `printf("%s", (${e}) ? "true" : "false");`;
  if (t === "Str") return 'printf("\\\"%s\\\"", ' + e + ');';
  if (t.startsWith("[") && t.endsWith("]")) return `h_print_${shortName(t.slice(1, -1))}_Array(&${e});`;
  if (ctx.classes[t]) return `if (${e}) { h_print_${t}(${e}); } else { printf("null"); }`;   // ref 字段可能为 NULL（目标已销毁被通知置空）
  if (ctx.structs[t]) return `h_print_${t}(&${e});`;
  return `printf("?");`;   // 枚举等：占位（示例避开）
}

/* 类方法表提升（与求值器一致的简化版：自己 + 导入深度传递，hide/alias） */
function computeClassMethods(classes) {
  const cache = {};
  const resolve = (clsName, visiting) => {
    if (cache[clsName]) return cache[clsName];
    if (visiting.has(clsName)) return cache[clsName] || {};
    visiting.add(clsName);
    const cls = classes[clsName];
    const table = {};
    for (const m of cls.methods) table[m.name] = { source: clsName, name: m.name };
    for (const imp of cls.imports) {
      const sub = resolve(imp.name, visiting);
      for (const [n, entry] of Object.entries(sub)) if (!table[n]) table[n] = entry;
    }
    for (const h of cls.hides) {
      if (h.path.parts.length >= 2) {
        const [src, mname] = h.path.parts;
        if (table[mname] && table[mname].source === src) delete table[mname];
      }
    }
    for (const al of cls.aliases) {
      const [src, mname] = al.path.parts;
      const sub = cache[src] || resolve(src, visiting);
      if (sub[mname]) table[al.alias] = sub[mname];
    }
    visiting.delete(clsName);
    cache[clsName] = table;
    return table;
  };
  const out = {};
  for (const n of Object.keys(classes)) out[n] = resolve(n, new Set());
  return out;
}

function genC(ast) {
  const enums = {}, rets = {}, structFields = {}, classes = {}, arrayElems = new Set(), paramKinds = {};
  const collectTypes = (t) => {
    if (!t) return;
    if (t.type === "ArrayType") { arrayElems.add(typeName(t.elem)); collectTypes(t.elem); }
  };
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
    if (e.type === "ArrayLiteral") { arrayElems.add(literalElemType(e.items)); e.items.forEach(scanExpr); return; }
    for (const v of Object.values(e)) {
      if (Array.isArray(v)) v.forEach(scanExpr);
      else if (v && typeof v === "object" && v.type) scanExpr(v);
    }
  };
  for (const d of ast.decls) {
    if (d.type === "EnumDecl") enums[d.name] = d.variants;
    if (d.type === "FunDecl") {
      rets[d.name] = d.ret ? typeName(d.ret.rtype) : "void";
      paramKinds[d.name] = d.params.map(p => p.kind);
      collectTypes(d.ret && d.ret.rtype);
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
    if (d.type === "ClassDecl") {
      const fields = [];
      for (const f of d.fields) { fields.push({ name: f.name, type: typeName(f.fieldType), fieldType: f.fieldType }); collectTypes(f.fieldType); }
      classes[d.name] = { fields, methods: d.methods, imports: d.imports, hides: d.hides, aliases: d.aliases };
      for (const m of d.methods) {
        paramKinds[d.name + "_" + m.name] = m.params.map(p => p.kind);
        collectTypes(m.ret && m.ret.rtype);
        for (const p of m.params) collectTypes(p.ptype);
        m.body.stmts.forEach(scanExpr);
      }
    }
  }
  const ctx = { enums, rets, structs: structFields, classes, classMethods: computeClassMethods(classes), paramKinds };
  const arrayDefs = [...arrayElems].map(e =>
    `typedef struct { unsigned long long len; ${cType(e)}* data; } ${shortName(e)}_Array;`
  ).join("\n");

  const structs = [], enumsDefs = [], classDefs = [], funcs = [];
  for (const d of ast.decls) {
    if (d.type === "StructDecl") structs.push(genStruct(d));
    else if (d.type === "EnumDecl") enumsDefs.push(genEnum(d, ctx));
    else if (d.type === "ClassDecl") classDefs.push(genClass(d, ctx));
    else if (d.type === "FunDecl") funcs.push(genFun(d, ctx, null));
    else if (d.type === "InterfaceDecl") { /* 契约，不生成 */ }
    else if (d.type === "GlobalDecl") throw new Error("C 后端暂不支持 global/并发——请用 h run");
    else if (d.type === "SpawnStmt" || d.type === "YieldStmt") throw new Error("C 后端暂不支持并发——请用 h run");
    else if (d.type === "ExprStmt") {
      const isMainCall = d.expr.type === "CallExpr" && d.expr.callee.type === "Ident" && d.expr.callee.name === "main";
      if (!isMainCall) throw new Error("C 后端暂不支持顶层语句——请用 h run");
    }
  }
  // 打印函数（方案 b：与解释器 valueToStr 逐字一致）——struct/数组/class 全量生成
  const printFns = [];
  for (const n of Object.keys(structFields)) printFns.push(genPrintStruct(n, ctx));
  for (const e of arrayElems) printFns.push(genPrintArray(e, ctx));
  for (const n of Object.keys(classes)) printFns.push(genPrintClass(n, ctx));
  const printProtos = printFns.map(f => f.split("{")[0].trim() + ";");
  // 类型顺序：前向声明（struct/class tag）→ enum → 数组 → struct 定义 → class 定义 → 打印/方法/函数
  const fwdDecls = [];
  for (const n of Object.keys(structFields)) fwdDecls.push(`typedef struct ${n} ${n};`);
  for (const n of Object.keys(classes)) fwdDecls.push(`typedef struct ${n} ${n};`);
  const typeDefs = structs.concat(genClassDecls(ast, ctx));
  const body = [
    '#include <stdio.h>',
    '#include <stdbool.h>',
    '#include <stdint.h>',
    '#include <stdlib.h>',
    '#include <string.h>',
    '',
    '/* 双向引用通知：树对象内嵌被引用链表，销毁时通知所有 ref 字段置 NULL */',
    'struct h_ref_link { void** pslot; struct h_ref_link* next; };',
    'static void h_ref_detach(struct h_ref_link** head, struct h_ref_link* ln) {',
    '  struct h_ref_link** p = head;',
    '  while (*p && *p != ln) p = &(*p)->next;',
    '  if (*p) *p = ln->next;',
    '  ln->next = NULL;',
    '}',
    '',
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
  return body.concat(fwdDecls, [""], enumsDefs, arrayDefs ? [arrayDefs, ""] : [], typeDefs, [""],
    printProtos, [""], printFns, [""], classDefs, funcs,
    ["int main(void) {", "  h_main();", "  return 0;", "}", ""]).join("\n");
}

function genStruct(d) {
  const fields = d.fields.map(f => `  ${cType(typeName(f.fieldType))} ${f.name};`).join("\n");
  return `struct ${d.name} {\n${fields}\n};`;   // typedef 由 fwdDecls 提供（避免匿名 struct 重定义 typedef）
}
function genClassDecls(ast, ctx) {
  return ast.decls.filter(d => d.type === "ClassDecl").map(d => {
    const refs = d.fields.filter(f => f.fieldType.mutable);
    const fields = d.fields.map(f => {
      const ft = typeName(f.fieldType);
      const ct = f.fieldType.mutable ? cType(ft) + "*" : cType(ft);
      return `  ${ct} ${f.name};`;
    }).join("\n");
    const links = refs.map(f => `  struct h_ref_link _${f.name}_link;`).join("\n");
    return `struct ${d.name} {\n${fields}\n  struct h_ref_link* _refs;\n${links ? links + "\n" : ""}};`;
  });
}

/* ---------- 打印函数（与求值器 valueToStr 逐字一致） ---------- */
function genPrintStruct(name, ctx) {
  let body = "", first = true;
  for (const [fname, ftype] of Object.entries(ctx.structs[name])) {
    if (!first) body += `    printf(", ");\n`;
    first = false;
    body += `    printf("${fname}: ");\n    ` + scalarPrint(ftype, `p->${fname}`, ctx) + "\n";
  }
  return `static void h_print_${name}(const ${name}* p) {\n  printf("${name}{");\n${body}  printf("}");\n}`;
}
function genPrintClass(name, ctx) {
  let body = "", first = true;
  for (const f of ctx.classes[name].fields) {
    if (!first) body += `    printf(", ");\n`;
    first = false;
    body += `    printf("${f.name}: ");\n    ` + scalarPrint(f.type, `p->${f.name}`, ctx) + "\n";
  }
  return `static void h_print_${name}(const ${name}* p) {\n  printf("${name}{");\n${body}  printf("}");\n}`;
}
function genPrintArray(elem, ctx) {
  const an = shortName(elem) + "_Array";
  const el = scalarPrint(elem, "a->data[i]", ctx);
  return `static void h_print_${an}(const ${an}* a) {\n  printf("[");\n  for (unsigned long long i = 0; i < a->len; i++) {\n    if (i) printf(", ");\n    ${el}\n  }\n  printf("]");\n}`;
}
function genEnum(d) {
  const names = d.variants.map(v => `  ${d.name}_${v}`).join(",\n");
  return `typedef enum {\n${names}\n} ${d.name};`;
}

/* ---------- class（树）：构造/释放/引用通知 + 方法（typedef 见 genClassDecls） ---------- */
function genClass(d, ctx) {
  const refFields = d.fields.filter(f => f.fieldType.mutable);
  // 构造：h_new_Type(字段按声明序)；数组字段深拷贝；ref 字段经 setter 注册（双向引用）
  const newArgs = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    const ct = f.fieldType.mutable ? cType(ft) + "*" : cType(ft);
    return `${ct} ${f.name}_v`;
  }).join(", ");
  const newInit = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    if (f.fieldType.mutable) return `  p->${f.name} = NULL;\n  h_set_${d.name}_${f.name}(p, ${f.name}_v);`;
    if (ft.startsWith("[") && ft.endsWith("]")) {
      const et = cType(ft.slice(1, -1));
      return `  p->${f.name} = ${f.name}_v;\n  p->${f.name}.data = (${et}*)malloc(sizeof(${et}) * ${f.name}_v.len);\n  memcpy(p->${f.name}.data, ${f.name}_v.data, sizeof(${et}) * ${f.name}_v.len);`;
    }
    return `  p->${f.name} = ${f.name}_v;`;
  }).join("\n");
  const ctor = `static ${d.name}* h_new_${d.name}(${newArgs || "void"}) {\n  ${d.name}* p = (${d.name}*)malloc(sizeof(${d.name}));\n  p->_refs = NULL;\n${newInit}\n  return p;\n}`;
  // setter：先注销旧目标，再注册到新目标（ref 字段双向引用）
  const setters = refFields.map(f => {
    const t = typeName(f.fieldType);
    return `static void h_set_${d.name}_${f.name}(${d.name}* h, ${t}* v) {\n  if (h->${f.name}) h_ref_detach(&h->${f.name}->_refs, &h->_${f.name}_link);\n  h->${f.name} = v;\n  if (v) { h->_${f.name}_link.pslot = (void**)&h->${f.name}; h->_${f.name}_link.next = v->_refs; v->_refs = &h->_${f.name}_link; }\n}`;
  }).join("\n");
  // 析构：通知所有指向本对象的 ref 字段置 NULL（防悬垂）；再注销本对象持有的 ref 字段
  const notify = `  struct h_ref_link* l = p->_refs;\n  while (l) { struct h_ref_link* nx = l->next; *(void**)l->pslot = NULL; l = nx; }`;
  const detaches = refFields.map(f => `  if (p->${f.name}) h_ref_detach(&p->${f.name}->_refs, &p->_${f.name}_link);`).join("\n");
  const dtorFrees = d.fields.map(f => {
    const ft = typeName(f.fieldType);
    return ft.startsWith("[") && ft.endsWith("]") ? `  free(p->${f.name}.data);` : "";
  }).filter(Boolean).join("\n");
  const dtor = `static void h_free_${d.name}(${d.name}* p) {\n${notify}\n${detaches}${detaches ? "\n" : ""}${dtorFrees}${dtorFrees ? "\n" : ""}  free(p);\n}`;
  const methods = d.methods.map(m => genFun(m, ctx, d.name)).join("\n");
  return (setters ? setters + "\n" : "") + ctor + "\n" + dtor + (methods ? "\n" + methods : "");
}

/* ---------- 函数 / 方法 ---------- */
function genFun(d, ctx, className) {
  const scope = new Scope(null, ctx);
  if (className) scope.receiverType = className;
  const params = d.params.map(p => {
    const t = typeName(p.ptype);
    let ct = ctx.classes[t] ? cType(t) + "*" : cType(t);
    if (p.kind === "ref") { scope.refParams.add(p.name); ct += "*"; }   // ref 参数 = 指向调用者变量的指针（写透别名）
    // 树参数语义：val/ref = 视图（不拥有，不销毁）；move = 拥有（函数退出时销毁）
    scope.declareType(p.name, t, p.kind === "move");
    return `${ct} ${p.name}`;
  });
  const allParams = (className ? [`${className}* self`] : []).concat(params);
  const body = d.body.stmts.map(s => genStmt(s, scope)).join("\n");
  // 函数最外层作用域：树变量销毁（move 后跳过——已从 trees 移除；视图参数从不进入）
  const frees = [...scope.trees].map(n => `  h_free_${scope.typeOf(n)}(${n});`).join("\n");
  const retT = d.ret ? typeName(d.ret.rtype) : "void";
  const ret = d.ret ? (ctx.classes[retT] ? cType(retT) + "*" : cType(retT)) : "void";
  let fname = className ? className + "_" + d.name : d.name;
  if (!className && fname === "main") fname = "h_main";
  return `${ret} ${fname}(${allParams.join(", ")}) {\n${body}${frees ? "\n" + frees : ""}\n}`;
}

class Scope {
  constructor(parent, ctx) {
    this.parent = parent || null; this.ctx = ctx;
    this.vars = new Set(); this.types = new Map(); this.trees = new Set(); this.receiverType = null; this.refParams = new Set();
  }
  declared(name) { let s = this; while (s) { if (s.vars.has(name)) return true; s = s.parent; } return false; }
  declareType(name, t, owned = true) { this.vars.add(name); this.types.set(name, t); if (owned && this.classType(t)) this.trees.add(name); }
  typeOf(name) { let s = this; while (s) { if (s.types.has(name)) return s.types.get(name); s = s.parent; } return null; }
  releaseTree(name) { let s = this; while (s) { if (s.trees.delete(name)) return; s = s.parent; } }
  classType(t) { return t && this.ctx.classes[t] ? t : null; }
}

function genStmt(st, scope) {
  switch (st.type) {
    case "VarDecl": {
      if (st.kind === "ref" || st.kind === "move") throw new Error("C 后端暂不支持 ref/move 参数——请用 h run");
      const init = genExpr(st.init, scope);
      if (scope.declared(st.name)) {
        // 覆盖声明 = 赋值；ref 参数写透调用者变量
        const tgt = scope.refParams.has(st.name) ? "(*" + st.name + ")" : st.name;
        return `  ${tgt} = ${init};`;
      }
      const t = inferType(st.init, scope) || "void";
      scope.declareType(st.name, t);
      const ct = scope.classType(t) ? cType(t) + "*" : cType(t);
      return `  ${ct} ${st.name} = ${init};`;
    }
    case "ReturnStmt": {
      // 返回树（含 -> move T 的无 move 关键字逃逸）：所有权随返回值转移，函数退出不销毁
      if (st.expr && st.expr.type === "Ident") {
        const t = scope.typeOf(st.expr.name);
        if (scope.classType(t)) scope.releaseTree(st.expr.name);
      }
      return "  return " + (st.expr ? genExpr(st.expr, scope) : "") + ";";
    }
    case "IfStmt": {
      const c = genExpr(st.cond, scope);
      const t = genBlockInner(st.then, scope);
      if (st.els) {
        const e = st.els.type === "Block" ? genBlockInner(st.els, scope) : genStmt(st.els, scope);
        return `  if (${c}) {\n${t}\n  } else {\n${e}\n  }`;
      }
      return `  if (${c}) {\n${t}\n  }`;
    }
    case "Block": return genBlockInner(st, scope);
    case "ExprStmt":
      return "  " + genExpr(st.expr, scope) + ";";
    default:
      throw new Error("C 后端暂不支持语句 " + st.type);
  }
}
function genBlockInner(block, scope) {
  const inner = new Scope(scope, scope.ctx);
  inner.receiverType = scope.receiverType;   // 方法体内嵌套块仍可裸访问 self 字段
  const body = block.stmts.map(s => genStmt(s, inner)).join("\n");
  const frees = [...inner.trees].map(n => `  h_free_${inner.typeOf(n)}(${n});`).join("\n");
  return body + (frees ? "\n" + frees : "");
}

function genExpr(e, scope) {
  switch (e.type) {
    case "Literal": return cLiteral(e);
    case "Ident": {
      // ref 参数：解引用写透（调用者变量）
      if (scope.refParams.has(e.name)) return "(*" + e.name + ")";
      // 方法体内裸字段名 → self->field（变量优先）
      if (scope.receiverType && !scope.declared(e.name)) {
        const f = scope.ctx.classes[scope.receiverType].fields.find(x => x.name === e.name);
        if (f) return "self->" + e.name;
      }
      return e.name;
    }
    case "MemberExpr": {
      // 枚举值 Type.Variant → 常量名
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name + "_" + e.prop;
      const ot = inferType(e.obj, scope);
      // 数组 .len → .len 字段
      if (ot && ot.startsWith("[") && e.prop === "len") return genExpr(e.obj, scope) + ".len";
      // class（树）字段 → 指针解引用
      if (scope.classType(ot)) return genExpr(e.obj, scope) + "->" + e.prop;
      return genExpr(e.obj, scope) + "." + e.prop;
    }
    case "CallExpr": return genCall(e, scope);
    case "BinExpr": {
      const l = genExpr(e.left, scope), r = genExpr(e.right, scope);
      return `(${l} ${e.op} ${r})`;
    }
    case "UnaryExpr": return `(${e.op}${genExpr(e.operand, scope)})`;
    case "AssignExpr": {
      // ref 字段赋值 → setter（先注销旧目标、再注册新目标，双向引用通知）
      if (e.op === "=" && e.left.type === "MemberExpr") {
        const ot = inferType(e.left.obj, scope);
        const fld = scope.ctx.classes[ot] && scope.ctx.classes[ot].fields.find(x => x.name === e.left.prop);
        if (fld && fld.fieldType.mutable) {
          return `h_set_${ot}_${e.left.prop}(${genExpr(e.left.obj, scope)}, ${genExpr(e.right, scope)});`;
        }
      }
      const target = e.left.type === "Ident" ? genExpr(e.left, scope) : genExpr(e.left, scope);
      return `${target} ${e.op === "=" ? "=" : e.op} ${genExpr(e.right, scope)}`;
    }
    case "ConstructExpr": return genConstruct(e, scope);
    case "MatchExpr": return genMatch(e, scope);
    case "MoveExpr": {
      // move：所有权转移——源从销毁表移除（视图参数不在表内，无操作）
      if (e.expr.type === "Ident") { scope.releaseTree(e.expr.name); return e.expr.name; }
      return genExpr(e.expr, scope);
    }
    case "MoveExpr": {
      if (e.expr.type === "Ident") { scope.releaseTree(e.expr.name); return e.expr.name; }
      return genExpr(e.expr, scope);
    }
    case "ErrorLit": throw new Error("C 后端暂不支持 error——请用 h run");
    case "ArrayLiteral": return genArrayLiteral(e, inferType(e, scope), scope);
    case "IndexExpr":
      return genExpr(e.obj, scope) + ".data[" + genExpr(e.index, scope) + "]";
    default: throw new Error("C 后端暂不支持表达式 " + e.type);
  }
}

function genConstruct(e, scope) {
  const cls = scope.ctx.classes[e.name];
  if (cls) {
    // 树构造 → h_new_Type(字段按声明序)
    const args = cls.fields.map(f => {
      const found = e.fields.find(x => x.name === f.name);
      if (found) {
        const ft = typeName(f.fieldType);
        // 空数组字面量无元素类型信息 → 强制按字段声明类型生成
        if (ft.startsWith("[") && ft.endsWith("]") && found.expr.type === "ArrayLiteral") {
          return genArrayLiteral(found.expr, ft, scope);
        }
        return genExpr(found.expr, scope);
      }
      return "0";
    });
    return `h_new_${e.name}(${args.join(", ")})`;
  }
  const fields = e.fields.map(f => `.${f.name} = ${genExpr(f.expr, scope)}`).join(", ");
  return `(${e.name}){ ${fields} }`;
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
  return `({ __typeof__(${target}) _h_t = (${target}); ${retT} _h_m = 0; switch (_h_t) {\n${cases}\n    default: break; } _h_m; })`;
}

function genCall(e, scope) {
  const callee = e.callee;
  // print 特殊：参数走 genPrint（to_str 剥皮/树/struct 整值打印），勿预先 genExpr
  if (callee.type === "Ident" && callee.name === "print") return genPrint(e.args, scope);
  let fname = null;
  if (callee.type === "Ident") {
    if (callee.name === "store" || callee.name === "load") throw new Error("C 后端暂不支持 store/load——请用 h run");
    fname = callee.name === "main" ? "h_main" : callee.name;
  } else if (callee.type === "MemberExpr") {
    const t = inferType(callee.obj, scope);
    const table = scope.ctx.classMethods[t];
    const entry = table && table[callee.prop];
    if (entry) fname = entry.source + "_" + entry.name;
    else if (callee.prop === "to_str") throw new Error("C 后端暂不支持 to_str（print 参数内自动处理）——请用 h run");
    else throw new Error("C 后端暂不支持方法调用 " + callee.prop);
  } else throw new Error("C 后端暂不支持该调用");
  const kinds = scope.ctx.paramKinds[fname];
  const args = e.args.map((a, i) => {
    if (kinds && kinds[i] === "ref") {
      // ref 参数：实参必须是可写变量 → 传地址（写透别名）
      if (a.type !== "Ident") throw new Error("ref 实参必须是可写变量（checker R3）——请用 h run");
      return "&" + genExpr(a, scope);
    }
    return genExpr(a, scope);
  });
  if (callee.type === "Ident") return `${fname}(${args.join(", ")})`;
  return `${fname}(${genExpr(callee.obj, scope)}${args.length ? ", " + args.join(", ") : ""})`;
}

function genPrint(args, scope) {
  const parts = [];
  for (const a of args) {
    if (parts.length) parts.push('printf(" ");');
    let t = inferType(a, scope);
    // x.to_str()：按 x 本身的类型打印（求值器 to_str = valueToStr，字符串会加引号）——先剥皮再生成代码
    let target = a, fromToStr = false;
    if (a.type === "CallExpr" && a.callee.type === "MemberExpr" && a.callee.prop === "to_str") {
      target = a.callee.obj;
      t = inferType(target, scope);
      fromToStr = true;
    }
    const code = genExpr(target, scope);
    if (t === "f64") parts.push(`h_print_f64(${code});`);
    else if (t === "u64") parts.push(`printf("%llu", ${code});`);
    else if (t === "bool") parts.push(`printf("%s", (${code}) ? "true" : "false");`);
    else if (t === "Str") parts.push(fromToStr ? `printf("\\\"%s\\\"", ${code});` : `printf("%s", ${code});`);
    else if (scope.classType(t)) parts.push(`if (${code}) { h_print_${t}(${code}); } else { printf("null"); }`);
    else if (scope.ctx.structs[t]) parts.push(`h_print_${t}(&${code});`);
    else if (t && t.startsWith("[") && t.endsWith("]")) parts.push(`h_print_${shortName(t.slice(1, -1))}_Array(&${code});`);
    else parts.push(`printf("%s", ${code});`);
  }
  parts.push('printf("\\n");');
  return parts.join("\n  ");
}

/* 数组字面量 → 复合字面量（可强制元素类型：空数组/字段类型已知时） */
function genArrayLiteral(e, t, scope) {
  const items = e.items.map(x => genExpr(x, scope)).join(", ");
  const et = cType(t.slice(1, -1));
  return `(${cType(t)}){ .len = ${e.items.length}, .data = (${et}[]){ ${items} } }`;
}

function inferEnumName(e, scope) {
  if (e.type === "Ident") {
    const t = scope.typeOf(e.name);
    return t && t !== "?" && t !== "void" && !scope.classType(t) ? t : null;
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
    case "MoveExpr": return inferType(e.expr, scope);
    case "MemberExpr": {
      if (e.obj.type === "Ident" && !scope.declared(e.obj.name)) return e.obj.name;
      const ot = inferType(e.obj, scope);
      if (ot && ot.startsWith("[") && e.prop === "len") return "u64";
      if (scope.ctx.classes[ot]) {
        const f = scope.ctx.classes[ot].fields.find(x => x.name === e.prop);
        return f ? f.type : "?";
      }
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
      if (e.callee.type === "MemberExpr") {
        const t = inferType(e.callee.obj, scope);
        const table = scope.ctx.classMethods[t];
        const entry = table && table[e.callee.prop];
        if (entry) {
          const src = scope.ctx.classes[entry.source];
          const m = src.methods.find(x => x.name === entry.name);
          return m && m.ret ? typeName(m.ret.rtype) : "void";
        }
      }
      return "?";
    }
    default: return "?";
  }
}

module.exports = { genC };
