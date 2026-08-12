// H 语言求值器 + 协作式调度器（执行体 = 生成器协程）
// 并发：spawn 入队 / yield 让出 / Channel 通信（send 满、recv 空挂起）

const { parse } = require("./parser");

function valueToStr(v) {
  if (v === null || v === undefined) return "null";
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  if (typeof v === "string") return '"' + v + '"';
  if (Array.isArray(v)) return "[" + v.map(valueToStr).join(", ") + "]";
  if (v.__shape === "void") return "void";
  if (v.__shape === "error") return "error." + v.__name;
  if (v.__shape === "enum") return v.__type + "." + v.__variant;
  if (v.__shape === "channel") return "Channel(" + v.__chan.cap + ")";
  if (v.__kind === "tuple") {
    const named = v.__names.length && !/^\d+$/.test(v.__names[0]);
    const parts = v.__names.map((n, i) => named ? n + ": " + valueToStr(v.__items[i]) : valueToStr(v.__items[i]));
    return "(" + parts.join(", ") + ")";
  }
  if (v.__shape === "slice") return "[" + v.__arr.slice(v.__start, v.__start + v.__len).map(valueToStr).join(", ") + "]";
  if (v.__shape === "optional") return v.__has ? valueToStr(v.__value) : "null";
  if (v.__shape === "func") return "fun." + (v.__fn.name || "闭包");
  const alive = v.__alive === undefined || v.__alive ? "" : " 💀";
  const inner = Object.entries(v.__fields || {}).map(([k, x]) => k + ": " + valueToStr(x)).join(", ");
  return v.__type + "{" + inner + "}" + alive;
}

/* ---------- Channel：容量、缓冲、等待者队列 ---------- */
class Channel {
  constructor(cap) { this.buf = []; this.cap = Math.max(1, cap || 1); this.waitSend = []; this.waitRecv = []; this.id = 0; }
  trySend(v) { if (this.buf.length < this.cap) { this.buf.push(v); return true; } return false; }
  tryRecv() { if (this.buf.length) return { ok: true, value: this.buf.shift() }; return { ok: false }; }
}

/* ---------- 宿主接口：print/channel/spawn 的可插拔 I/O ---------- */
class LocalHost {
  constructor(ev) { this.ev = ev; }
  print(text) { this.ev.output.push(text); this.ev.event("output", { text }); }
  log(text) { this.ev.output.push(text); this.ev.event("output", { text }); }
  registerChannel(ch) { if (this.ev.sched) this.ev.sched.channels.push(ch); }
  *channelSend(ch, value) {
    if (ch.trySend(value)) return true;
    yield { k: "send", chan: ch, value };
    return true;
  }
  *channelRecv(ch) {
    const r = ch.tryRecv();
    if (r.ok) return r.value;
    const v = yield { k: "recv", chan: ch };
    return v;
  }
  *spawn(fn, argVals, loc) {
    const gen = this.ev.execBody(fn, argVals, loc, null, false);
    this.ev.sched.push(gen, fn.name, this.ev.current);
    this.ev.event("spawn", { name: fn.name });
    return { __shape: "void" };
  }
}

/* ---------- 调度器：就绪队列 + 等待唤醒 ---------- */
class Scheduler {
  constructor(ev) {
    this.ev = ev;
    this.ready = [];      // {gen, name, current, resumeValue}
    this.channels = [];
  }
  push(gen, name, current) { this.ready.push({ gen, name, current, resumeValue: undefined }); }
  run() {
    while (true) {
      while (this.ready.length) {
        const task = this.ready.shift();
        // 切换执行体上下文：恢复该执行体的作用域栈顶
        const saved = this.ev.current;
        this.ev.current = task.current;
        let r;
        try {
          r = task.gen.next(task.resumeValue);
        } finally {
          task.current = this.ev.current;
          this.ev.current = saved;
        }
        if (r.done) { this.ev.event("exec_done", { name: task.name }); continue; }
        const s = r.value;
        if (s.k === "yield") this.ready.push(task);
        else if (s.k === "recv") s.chan.waitRecv.push(task);
        else if (s.k === "send") { task.value = s.value; s.chan.waitSend.push(task); }
        else if (s.k === "wait_ext") { this.ev.pendingExt.set(s.id, task); }
      }
      // 唤醒等待者
      let woke = false;
      for (const ch of this.channels) {
        while (ch.waitRecv.length && ch.buf.length) {
          const t = ch.waitRecv.shift();
          t.resumeValue = ch.buf.shift();
          this.ready.push(t); woke = true;
        }
        while (ch.waitSend.length && ch.buf.length < ch.cap) {
          const t = ch.waitSend.shift();
          ch.buf.push(t.value);
          t.resumeValue = true;
          this.ready.push(t); woke = true;
        }
      }
      if (!this.ready.length) break;
    }
  }
  wakeExt(taskId, result) {
    const task = this.ev.pendingExt.get(taskId);
    if (task) {
      this.ev.pendingExt.delete(taskId);
      task.resumeValue = result;
      this.ready.push(task);
    }
  }
}

class Scope {
  constructor(parent, name) { this.parent = parent; this.name = name; this.vars = {}; this.children = []; this.receiver = null; this.receiverWritable = false; }
  lookup(name) { let s = this; while (s) { if (name in s.vars) return s.vars[name]; s = s.parent; } return null; }
  findReceiver(name) {
    let s = this;
    while (s) {
      if (s.receiver && s.receiver.__fields && name in s.receiver.__fields) {
        return { receiver: s.receiver, writable: s.receiverWritable, scope: s };
      }
      s = s.parent;
    }
    return null;
  }
}

class Evaluator {
  constructor(ast, host) {
    this.ast = ast;
    this.types = {};
    this.funcs = {};
    this.global = new Scope(null, "全局");
    this.current = this.global;
    this.events = [];
    this.output = [];
    this.halted = false;
    this.haltMsg = null;
    this.classMethods = {};
    this.sched = null;
    this.channelSeq = 0;
    this.pendingExt = new Map();   // 外部等待（wait_ext）taskId -> task
    this.host = host || new LocalHost(this);
  }
  rerr(msg, loc) { throw { runtime: true, msg, line: loc ? loc.line : 0, col: loc ? loc.col : 0 }; }
  event(t, data) {
    this.events.push(Object.assign({ t }, data, { snap: this.snapshot() }));
  }
  snapshot() {
    const chain = [];
    let s = this.current;
    while (s) {
      chain.push({
        name: s.name,
        vars: Object.entries(s.vars).map(([n, v]) => ({
          n, mutable: v.mutable, alive: v.alive, kind: v.kind,
          val: v.kind === "ref"
            ? (v.target && v.target.alive ? "→ " + v.target.name : "→ 💀")
            : (v.alive ? valueToStr(v.value) : "已失效"),
        })),
      });
      s = s.parent;
    }
    return chain;
  }

  register() {
    for (const d of this.ast.decls) {
      if (d.type === "StructDecl") {
        const fields = {};
        for (const f of d.fields) fields[f.name] = { fieldType: f.fieldType, isMut: f.isMut };
        this.types[d.name] = { shape: "block", kind: "struct", fields };
      } else if (d.type === "ClassDecl") {
        const fields = {};
        for (const f of d.fields) fields[f.name] = { fieldType: f.fieldType, isMut: f.isMut };
        this.types[d.name] = { shape: "tree", kind: "class", fields, methods: d.methods, interfaces: d.interfaces, imports: d.imports, hides: d.hides, aliases: d.aliases };
      } else if (d.type === "EnumDecl") {
        this.types[d.name] = { shape: "block", kind: "enum", fields: {}, variants: d.variants };
      } else if (d.type === "FunDecl") {
        this.funcs[d.name] = d;
      }
    }
    this.computeClassMethods();
  }

  /* 方法表提升：自己的 + 导入的（深度传递）− 隐藏 + 别名；循环拒绝 */
  computeClassMethods() {
    const cache = {};
    const resolve = (clsName, visiting) => {
      if (cache[clsName]) return cache[clsName];
      if (visiting.has(clsName)) {
        const e = { runtime: true, msg: "class 导入循环：'" + clsName + "'", line: 0, col: 0 };
        throw e;
      }
      visiting.add(clsName);
      const cls = this.types[clsName];
      const table = {};
      for (const m of cls.methods) table[m.name] = { func: m, source: clsName };
      for (const imp of cls.imports) {
        const sub = resolve(imp.name, visiting);
        for (const [n, entry] of Object.entries(sub)) {
          if (!table[n]) table[n] = entry;
        }
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
    for (const n of Object.keys(this.types)) {
      if (this.types[n].kind === "class") this.classMethods[n] = resolve(n, new Set());
    }
  }

  /* ---------- 运行入口：主执行体 + 调度器 ---------- */
  run() {
    this.event("start", {});
    this.sched = new Scheduler(this);
    const main = this.execTopLevel();
    this.sched.push(main, "main", this.current);
    this.sched.run();
    this.event("end", { halted: this.halted, haltMsg: this.haltMsg });
  }

  *execTopLevel() {
    let calledMain = false;
    for (const d of this.ast.decls) {
      if (this.halted) break;
      if (d.type === "GlobalDecl") {
        let init = d.init ? yield* this.evalExpr(d.init) : null;
        const val = deepCopyBlock(init);
        this.global.vars[d.name] = { kind: "val", value: val, mutable: true, alive: true, owned: true };
        this.event("create_var", { name: d.name, mutable: true, val: valueToStr(val) });
      } else if (d.type === "SpawnStmt") {
        yield* this.spawnExec(d.callee);
      } else if (d.type === "ExprStmt") {
        if (d.expr.type === "CallExpr" && d.expr.callee.type === "Ident" && d.expr.callee.name === "main") calledMain = true;
        const r = yield* this.evalExpr(d.expr);
        if (this.isError(r)) this.haltWithError(r, d.expr.loc);
      } else if (d.type === "FunDecl" || d.type === "StructDecl" || d.type === "ClassDecl" || d.type === "EnumDecl" || d.type === "InterfaceDecl") {
        /* 声明不执行 */
      } else {
        yield* this.execStmt(d);
      }
    }
    // 入口语义：main 若定义且未被显式调用 → 自动调用（与编译后端一致）
    if (!calledMain && this.funcs["main"]) {
      yield* this.callFunction("main", [], [], null);
    }
    this.event("main_done", {});
  }

  /* spawn：创建执行体（生成器），入队，不立即执行 */
  *spawnExec(expr) {
    if (expr.type !== "CallExpr" || expr.callee.type !== "Ident") this.rerr("spawn 只能启动函数调用", expr.loc);
    const fn = this.funcs[expr.callee.name];
    if (!fn) this.rerr("未定义的函数 '" + expr.callee.name + "'", expr.loc);
    const argVals = [];
    for (const a of expr.args) argVals.push(yield* this.evalExpr(a));
    this.event("spawn", { name: fn.name });
    return yield* this.host.spawn(fn, argVals, expr.loc);
  }

  /* ---------- 语句 ---------- */
  *execStmt(st) {
    if (this.halted) return null;
    switch (st.type) {
      case "VarDecl": return yield* this.execVarDecl(st);
      case "ReturnStmt": {
        let val = null;
        if (st.expr) val = yield* this.evalExpr(st.expr);
        else val = { __shape: "void" };
        this.event("return_stmt", { val: valueToStr(val) });
        return { flow: "return", value: val };
      }
      case "IfStmt": {
        const c = yield* this.evalExpr(st.cond);
        if (truthy(c)) return yield* this.execBlock(st.then, new Scope(this.current, "if"));
        if (st.els) {
          if (st.els.type === "Block") return yield* this.execBlock(st.els, new Scope(this.current, "if"));
          return yield* this.execStmt(st.els);
        }
        return null;
      }
      case "ForStmt": return yield* this.execFor(st);
      case "WhileStmt": return yield* this.execWhile(st);
      case "BreakStmt": return { flow: "break" };
      case "ContinueStmt": return { flow: "continue" };
      case "Block": return yield* this.execBlock(st, new Scope(this.current, "块"));
      case "ExprStmt": {
        const r = yield* this.evalExpr(st.expr);
        if (this.isError(r)) this.haltWithError(r, st.expr.loc);
        return null;
      }
      case "SpawnStmt": yield* this.spawnExec(st.callee); return null;
      case "YieldStmt": yield { k: "yield" }; this.event("yield", {}); return null;
      default: return null;
    }
  }
  *execFor(st) {
    const start = Number(yield* this.evalExpr(st.range.obj));
    const end = st.range.end ? Number(yield* this.evalExpr(st.range.end)) : Number.MAX_SAFE_INTEGER;
    for (let i = start; i < end; i++) {
      const sc = new Scope(this.current, "for");
      sc.vars[st.varName] = { kind: "val", value: i, mutable: false, alive: true, owned: true };
      const r = yield* this.execBlock(st.body, sc);
      if (r && r.flow === "break") break;
      if (r && r.flow === "return") return r;
      // continue / 正常结束 → 下一次迭代
    }
    return null;
  }
  *execWhile(st) {
    while (truthy(yield* this.evalExpr(st.cond))) {
      const r = yield* this.execBlock(st.body, new Scope(this.current, "while"));
      if (r && r.flow === "break") break;
      if (r && r.flow === "return") return r;
    }
    return null;
  }
  *execBlock(block, scope) {
    this.enterScope(scope);
    let ret = null;
    for (const st of block.stmts) {
      if (this.halted) break;
      const r = yield* this.execStmt(st);
      if (r && r.flow) { ret = r; break; }   // return / break / continue 都停止本块（作用域照常退出）
    }
    this.exitScope(scope, ret && ret.value);
    return ret;
  }
  enterScope(sc) {
    sc.parent = this.current;
    this.current.children.push(sc);
    this.current = sc;
    this.event("enter_scope", { name: sc.name });
  }
  exitScope(sc, escapedValue) {
    const destroyed = [];
    for (const [vname, v] of Object.entries(sc.vars)) {
      if (v.kind === "ref") {
        if (v.alive) { v.alive = false; this.event("ref_destroyed", { name: vname }); }
        continue;
      }
      if (!v.alive) continue;
      if (!v.owned) { v.alive = false; this.event("view_released", { name: vname }); continue; }
      if (escapedValue && escapedValue.__shape === "tree" && v.value === escapedValue) continue;
      v.alive = false;
      if (v.value && v.value.__shape === "tree") {
        v.value.__alive = false;
        // 双向引用通知：所有指向本树的 ref 字段失效（置 null）——避免悬垂引用
        if (v.value.__refs && v.value.__refs.size) {
          for (const r of v.value.__refs) { if (r.holder && r.holder.__fields) r.holder.__fields[r.field] = null; }
          v.value.__refs.clear();
        }
      }
      destroyed.push(vname);
      this.invalidateRefsTo(v);
      this.event("destroy", { name: vname, shape: v.value && v.value.__shape === "tree" ? "树" : "块" });
    }
    this.current = sc.parent;
    this.event("exit_scope", { name: sc.name, destroyed });
  }
  invalidateRefsTo(targetVar) {
    const walk = (sc) => {
      for (const v of Object.values(sc.vars)) {
        if (v.kind === "ref" && v.alive && v.target === targetVar) {
          v.alive = false;
          this.event("ref_invalidated", { name: v.name, why: "目标已销毁/移动" });
        }
      }
      sc.children.forEach(walk);
    };
    walk(this.global);
  }
  haltWithError(v, loc) {
    this.halted = true;
    this.haltMsg = "error." + v.__name + "（未处理）";
    this.event("error_propagate", { name: v.__name, msg: this.haltMsg, line: loc ? loc.line : 0, col: loc ? loc.col : 0 });
  }
  isError(v) { return v && typeof v === "object" && v.__shape === "error"; }

  *execVarDecl(v) {
    if (v.kind === "ref") {
      if (!v.init || v.init.type !== "Ident") this.rerr("ref 声明要求可写变量", v.loc);
      const target = this.lookupVar(v.init.name);
      if (!target || !target.alive) this.rerr("ref 指向的变量不可用", v.loc);
      if (!target.mutable) this.rerr("只能对可写变量声明可写指针（ref）—— '" + v.init.name + "' 只读", v.loc);
      this.current.vars[v.name] = { kind: "ref", target, mutable: true, alive: true };
      this.event("create_ref", { name: v.name, target: v.init.name, mutable: target.mutable });
      return null;
    }
    const existing = this.current.lookup(v.name);
    if (v.kind === "val" && existing && existing.alive) {
      if (!existing.mutable) this.rerr("赋值目标不可写（只读变量）：'" + v.name + "'", v.loc);
      let val = v.init ? deepCopyBlock(yield* this.evalExpr(v.init)) : null;
      if (existing.opt) val = wrapOptVal(val);
      if (this.isError(val)) { this.haltWithError(val, v.loc); return null; }
      existing.value = val;
      this.event("assign", { target: v.name, op: "=", val: valueToStr(val) });
      return null;
    }
    let val = v.init ? deepCopyBlock(yield* this.evalExpr(v.init)) : null;
    if (v.annotation && v.annotation.type === "OptionalType") val = wrapOptVal(val);
    if (this.isError(val)) { this.haltWithError(val, v.loc); return null; }
    this.current.vars[v.name] = { kind: "val", value: val, mutable: v.kind === "mut", alive: true, owned: true, opt: !!(v.annotation && v.annotation.type === "OptionalType") };
    this.event("create_var", { name: v.name, mutable: v.kind === "mut", val: valueToStr(val) });
    return null;
  }

  /* ---------- 表达式 ---------- */
  *evalExpr(e) {
    if (!e) return { __shape: "void" };
    switch (e.type) {
      case "Literal": return e.ltype === "f32" ? Math.fround(e.value) : e.value;
      case "Ident": {
        const v = this.lookupVar(e.name);
        if (!v) {
          const rf = this.current.findReceiver(e.name);
          if (rf) return rf.receiver.__fields[e.name];
          if (this.funcs[e.name]) return { __shape: "func", __fn: this.funcs[e.name] };   // 函数名 → 函数引用值
          this.rerr("未定义的变量 '" + e.name + "'", e.loc);
        }
        if (!v.alive) this.rerr("变量 '" + e.name + "' 已失效（被 move 或所在作用域已退出）", e.loc);
        return v.kind === "ref" ? v.target.value : v.value;
      }
      case "MemberExpr": {
        if (e.obj.type === "Ident" && this.types[e.obj.name]) {
          const tdef = this.types[e.obj.name];
          if (tdef.kind === "enum") {
            if (!tdef.variants.includes(e.prop)) this.rerr("枚举 " + e.obj.name + " 没有变体 '" + e.prop + "'", e.loc);
            return { __shape: "enum", __type: e.obj.name, __variant: e.prop };
          }
          this.rerr("类型 '" + e.obj.name + "' 不能作为值（静态方法仅限内建）", e.loc);
        }
        const obj = yield* this.evalExpr(e.obj);
        if (Array.isArray(obj)) {
          if (e.prop === "len") return obj.length;
          this.rerr("动态块没有属性 '" + e.prop + "'（仅 len）", e.loc);
        }
        if (obj && obj.__shape === "slice") {
          if (e.prop === "len") return obj.__len;
          this.rerr("切片没有属性 '" + e.prop + "'（仅 len）", e.loc);
        }
        if (obj && obj.__kind === "tuple") {
          const idx = obj.__names.indexOf(e.prop);
          if (idx < 0) this.rerr("元组没有字段 '" + e.prop + "'", e.loc);
          return obj.__items[idx];
        }
        if (obj === null || obj === undefined || typeof obj !== "object" || obj.__shape === "void") this.rerr("无法访问字段 '" + e.prop + "'", e.loc);
        if (!(e.prop in obj.__fields)) this.rerr("类型 " + obj.__type + " 没有字段 '" + e.prop + "'", e.loc);
        return obj.__fields[e.prop];
      }
      case "CallExpr": return yield* this.execCall(e);
      case "BinExpr": {
        const l = yield* this.evalExpr(e.left), r = yield* this.evalExpr(e.right);
        let res;
        switch (e.op) {
          case "+": {
            const a = typeof l === "object" ? valueToStr(l) : l;
            const b = typeof r === "object" ? valueToStr(r) : r;
            if (typeof a === "string" || typeof b === "string") return String(a) + String(b);
            res = a + b;
            break;
          }
          case "-": res = l - r; break;
          case "*": res = l * r; break;
          // 整数整除（向零截断，对齐 C/Rust）；浮点或未知类型回退浮点
          case "/": res = isIntNum(e._t) ? Math.trunc(l / r) : l / r; break;
          case "%": res = isIntNum(e._t) ? Math.trunc(l % r) : l % r; break;
          case "==": return enumEq(l, r); case "!=": return !enumEq(l, r);
          case "<": return l < r; case "<=": return l <= r; case ">": return l > r; case ">=": return l >= r;
          case "&&": return truthy(l) && truthy(r); case "||": return truthy(l) || truthy(r);
          default: this.rerr("未知运算符 " + e.op, e.loc);
        }
        return e._t === "f32" ? Math.fround(res) : res;   // f32 单精度逐运算截断
      }
      case "UnaryExpr": {
        const x = yield* this.evalExpr(e.operand);
        if (e.op === "!") return !truthy(x);
        if (e.op === "-") return -x;
        this.rerr("未知一元运算符 " + e.op, e.loc);
        break;
      }
      case "MoveExpr": return yield* this.execMove(e);
      case "AssignExpr": return yield* this.execAssign(e);
      case "ConstructExpr": return yield* this.execConstruct(e);
      case "ErrorLit": return { __shape: "error", __name: e.name };
      case "MatchExpr": {
        const target = yield* this.evalExpr(e.target);
        if (!target || target.__shape !== "enum") this.rerr("match 目标必须是枚举值", e.loc);
        for (const arm of e.arms) {
          if (arm.variant === target.__variant) return yield* this.evalExpr(arm.expr);
        }
        this.rerr("match 未覆盖变体 " + target.__variant + "（穷尽性检查应在编译期拦截）", e.loc);
      }
      case "TupleLit": {
        const items = [];
        for (const it of e.items) items.push(deepCopyBlock(yield* this.evalExpr(it.expr)));
        const names = e.named ? e.items.map(i => i.name) : items.map((_, i) => String(i));
        return { __shape: "block", __kind: "tuple", __names: names, __items: items };
      }
      case "ClosureLit": {
        // 闭包求值：捕获环境（[x] 值复制 / [move y] 所有权转移）
        const captures = {};
        for (const c of e.captures) {
          const v = this.lookupVar(c.name);
          if (!v || !v.alive) this.rerr("捕获变量 '" + c.name + "' 不可用", e.loc);
          if (c.kind === "move") {
            v.alive = false;
            this.invalidateRefsTo(v);
            this.event("move", { name: c.name, val: valueToStr(v.value) });
            captures[c.name] = v.value;
          } else {
            captures[c.name] = deepCopyBlock(v.value);
          }
        }
        return { __shape: "func", __fn: e, __captures: captures };
      }
      case "RangeExpr": {
        const obj = yield* this.evalExpr(e.obj);
        const len = Array.isArray(obj) ? obj.length : obj && obj.__shape === "slice" ? obj.__len : null;
        if (len === null) this.rerr("取区间目标必须是动态块或切片", e.loc);
        const start = e.start ? Number(yield* this.evalExpr(e.start)) : 0;
        const end = e.end ? Number(yield* this.evalExpr(e.end)) : len;
        if (start < 0 || end > len || start > end) this.rerr("切片区间越界 " + start + ".." + end, e.loc);
        if (Array.isArray(obj)) return { __shape: "slice", __arr: obj, __start: start, __len: end - start };
        return { __shape: "slice", __arr: obj.__arr, __start: obj.__start + start, __len: end - start };
      }
      case "UnwrapExpr": {
        const v = yield* this.evalExpr(e.expr);
        if (v && typeof v === "object" && v.__shape === "optional") {
          if (!v.__has) this.rerr("解包 null（可选值无值）", e.loc);
          return v.__value;
        }
        if (v === null || v === undefined) this.rerr("解包 null（可选值无值）", e.loc);
        return v;
      }
      case "ArrayLiteral": {
        const out = [];
        for (const it of e.items) out.push(deepCopyBlock(yield* this.evalExpr(it)));
        return out;
      }
      case "IndexExpr": {
        const obj = yield* this.evalExpr(e.obj);
        const idx = yield* this.evalExpr(e.index);
        if (obj && obj.__shape === "slice") {
          if (idx < 0 || idx >= obj.__len) this.rerr("索引越界", e.loc);
          return obj.__arr[obj.__start + idx];
        }
        if (!Array.isArray(obj)) this.rerr("索引目标不是数组", e.loc);
        return obj[idx];
      }
      default: this.rerr("未知表达式 " + e.type, e.loc);
    }
  }

  *execMove(e) {
    if (e.expr.type === "Ident") {
      const v = this.lookupVar(e.expr.name);
      if (!v || !v.alive) this.rerr("变量 '" + e.expr.name + "' 已失效，不能 move", e.loc);
      v.alive = false;
      this.invalidateRefsTo(v);
      this.event("move", { name: e.expr.name, val: valueToStr(v.value) });
      return v.value;
    }
    return yield* this.evalExpr(e.expr);
  }

  *execAssign(e) {
    // 解构 (a, b) = expr：逐元素——声明新建或覆盖已有（R4 可写检查）
    if (e.left.type === "TupleLit") {
      const vals = yield* this.evalExpr(e.right);
      if (!vals || vals.__kind !== "tuple") this.rerr("解构右侧必须是元组", e.loc);
      if (vals.__items.length !== e.left.items.length) this.rerr("解构元素数不匹配（期望 " + e.left.items.length + "，实际 " + vals.__items.length + "）", e.loc);
      for (let i = 0; i < e.left.items.length; i++) {
        const it = e.left.items[i];
        if (!it.expr || it.expr.type !== "Ident") this.rerr("解构目标必须是变量名", e.loc);
        const v = this.lookupVar(it.expr.name);
        if (v) {
          if (!v.mutable) this.rerr("解构目标不可写（只读变量）：'" + it.expr.name + "'", e.loc);
          v.value = vals.__items[i];
          this.event("assign", { target: it.expr.name, op: "=", val: valueToStr(vals.__items[i]) });
        } else {
          this.current.vars[it.expr.name] = { kind: "val", value: vals.__items[i], mutable: !!it.mut, alive: true, owned: true };
          this.event("create_var", { name: it.expr.name, mutable: !!it.mut, val: valueToStr(vals.__items[i]) });
        }
      }
      return vals;
    }
    const lv = this.resolveLValue(e.left, e.loc);
    if (!lv.mutable) this.rerr("赋值目标不可写（只读变量 / 只读指针 / 非 mut 字段）", e.loc);
    let val = yield* this.evalExpr(e.right);
    if (e.op !== "=") {
      const cur = lv.get();
      const r = yield* this.evalExpr(e.right);
      val = binOp(e.op[0], cur, r);
    }
    lv.set(deepCopyBlock(val));
    this.event("assign", { target: lv.label, op: e.op, val: valueToStr(val) });
    return val;
  }

  resolveLValue(e, loc) {
    if (e.type === "Ident") {
      const v = this.lookupVar(e.name);
      if (!v) {
        const rf = this.current.findReceiver(e.name);
        if (rf) {
          const def = this.types[rf.receiver.__type];
          const f = def && def.fields[e.name];
          return {
            mutable: rf.writable && f && f.isMut, label: e.name,
            get: () => rf.receiver.__fields[e.name],
            set: (x) => { rf.receiver.__fields[e.name] = x; },
          };
        }
        this.rerr("未定义的变量 '" + e.name + "'", loc);
      }
      if (!v.alive) this.rerr("变量 '" + e.name + "' 已失效", loc);
      if (v.kind === "ref") {
        if (!v.target.alive) this.rerr("ref '" + e.name + "' 指向的数据已销毁", loc);
        const t = v.target;
        return {
          mutable: t.mutable, label: e.name + "→" + t.name,
          get: () => t.value, set: (x) => { t.value = x; },
        };
      }
      return { mutable: v.mutable, label: e.name, get: () => v.value, set: (x) => { v.value = v.opt ? wrapOptVal(x) : x; } };
    }
    if (e.type === "MemberExpr") {
      const obj = this.evalExprSync(e.obj);
      if (!obj || typeof obj !== "object" || !obj.__fields) this.rerr("无法写字段", loc);
      const def = this.types[obj.__type];
      const f = def && def.fields[e.prop];
      if (!f) this.rerr("类型 " + obj.__type + " 没有字段 '" + e.prop + "'", loc);
      const objWritable = this.objWritable(e.obj);
      const refEntry = { holder: obj, field: e.prop };
      return {
        mutable: objWritable && f.isMut, label: valueToStr(obj).split("{")[0] + "." + e.prop,
        get: () => obj.__fields[e.prop],
        set: (x) => {
          // ref 字段（双向引用）：写时注销旧目标、注册新目标
          const fd = def.fields[e.prop];
          if (fd && fd.fieldType.mutable) {
            const old = obj.__fields[e.prop];
            if (old && old.__shape === "tree" && old.__refs) old.__refs.delete(refEntry);
            if (x && x.__shape === "tree" && x.__refs) x.__refs.add(refEntry);
          }
          obj.__fields[e.prop] = x;
        },
      };
    }
    if (e.type === "IndexExpr") {
      const obj = this.evalExprSync(e.obj);
      const idx = Number(this.evalExprSync(e.index));
      const writable = this.objWritable(e.obj);
      if (obj && obj.__shape === "slice") {
        if (idx < 0 || idx >= obj.__len) this.rerr("索引越界", loc);
        return {
          mutable: writable, label: "切片[" + idx + "]",
          get: () => obj.__arr[obj.__start + idx], set: (x) => { obj.__arr[obj.__start + idx] = x; },
        };
      }
      if (Array.isArray(obj)) {
        return { mutable: writable, label: "[" + idx + "]", get: () => obj[idx], set: (x) => { obj[idx] = x; } };
      }
      this.rerr("无法写索引目标", loc);
    }
    this.rerr("无法解析赋值目标", loc);
  }
  evalExprSync(e) {
    // 赋值目标的左值求值——同步路径（不挂起）。仅处理无挂起风险的表达式，其余返回 void
    if (!e) return { __shape: "void" };
    if (e.type === "Literal") return e.value;
    if (e.type === "Ident") {
      const v = this.lookupVar(e.name);
      if (!v) {
        const rf = this.current.findReceiver(e.name);
        if (rf) return rf.receiver.__fields[e.name];
        this.rerr("未定义的变量 '" + e.name + "'", e.loc);
      }
      if (!v.alive) this.rerr("变量 '" + e.name + "' 已失效", e.loc);
      return v.kind === "ref" ? v.target.value : v.value;
    }
    if (e.type === "MemberExpr") {
      const obj = this.evalExprSync(e.obj);
      if (Array.isArray(obj)) {
        if (e.prop === "len") return obj.length;
        this.rerr("动态块没有属性 '" + e.prop + "'（仅 len）", e.loc);
      }
      if (obj && obj.__shape === "slice") {
        if (e.prop === "len") return obj.__len;
        this.rerr("切片没有属性 '" + e.prop + "'（仅 len）", e.loc);
      }
      if (obj && obj.__kind === "tuple") {
        const idx = obj.__names.indexOf(e.prop);
        if (idx < 0) this.rerr("元组没有字段 '" + e.prop + "'", e.loc);
        return obj.__items[idx];
      }
      if (!obj || !obj.__fields) this.rerr("无法访问字段", e.loc);
      return obj.__fields[e.prop];
    }
    if (e.type === "IndexExpr") {
      const obj = this.evalExprSync(e.obj);
      const idx = Number(this.evalExprSync(e.index));
      if (obj && obj.__shape === "slice") {
        if (idx < 0 || idx >= obj.__len) this.rerr("索引越界", e.loc);
        return obj.__arr[obj.__start + idx];
      }
      if (!Array.isArray(obj)) this.rerr("索引目标不是数组或切片", e.loc);
      return obj[idx];
    }
    if (e.type === "BinExpr") {
      const l = this.evalExprSync(e.left), r = this.evalExprSync(e.right);
      let res;
      switch (e.op) {
        case "+": res = (typeof l === "object" ? valueToStr(l) : l) + (typeof r === "object" ? valueToStr(r) : r); break;
        case "-": res = l - r; break;
        case "*": res = l * r; break;
        // 整数整除（向零截断，对齐 C/Rust）；浮点或未知类型回退浮点
        case "/": res = isIntNum(e._t) ? Math.trunc(l / r) : l / r; break;
        case "%": res = isIntNum(e._t) ? Math.trunc(l % r) : l % r; break;
        case "==": return enumEq(l, r); case "!=": return !enumEq(l, r);
        case "<": return l < r; case "<=": return l <= r; case ">": return l > r; case ">=": return l >= r;
        case "&&": return truthy(l) && truthy(r); case "||": return truthy(l) || truthy(r);
        default: this.rerr("未知运算符 " + e.op, e.loc);
      }
      return e._t === "f32" ? Math.fround(res) : res;
    }
    return { __shape: "void" };
  }
  objWritable(e) {
    if (e.type === "Ident") {
      const v = this.lookupVar(e.name);
      if (v && v.alive && v.kind === "ref") return v.target.mutable;
      return v && v.alive ? v.mutable : false;
    }
    if (e.type === "ConstructExpr") return true;
    if (e.type === "MemberExpr") { const o = this.evalExprSync(e); return !!o; }
    return false;
  }

  *execConstruct(e) {
    const def = this.types[e.name];
    if (!def) this.rerr("未定义的类型 '" + e.name + "'", e.loc);
    const fields = {};
    const refRegs = [];   // (字段名, 目标树)——ref 字段指向的树需要注册（双向引用）
    for (const [k, fd] of Object.entries(def.fields)) fields[k] = deepCopyBlock(defaultValue(fd.fieldType));
    for (const f of e.fields) {
      if (!(f.name in fields)) this.rerr("类型 " + e.name + " 没有字段 '" + f.name + "'", e.loc);
      const fd = def.fields[f.name];
      let fv = deepCopyBlock(yield* this.evalExpr(f.expr));
      if (fd && fd.fieldType.type === "OptionalType") fv = wrapOptVal(fv);   // 可选字段：裸值提升
      fields[f.name] = fv;
      const val = fields[f.name];
      if (fd && fd.fieldType.mutable && val && val.__shape === "tree" && val.__refs) refRegs.push([f.name, val]);
    }
    const inst = { __shape: def.shape, __type: e.name, __fields: fields, __alive: true, __refs: new Set() };
    for (const [fn, val] of refRegs) val.__refs.add({ holder: inst, field: fn });
    this.event("construct", { name: e.name, val: valueToStr(inst) });
    return inst;
  }

  *execCall(e) {
    const callee = e.callee;
    const args = [];
    for (const a of e.args) args.push(yield* this.evalExpr(a));
    if (callee.type === "MemberExpr") {
      const mname = callee.prop;
      const isStatic = callee.obj.type === "Ident" && this.types[callee.obj.name];
      if (isStatic) {
        if (mname === "from_bytes") return this.fromBytes(args[0], callee.obj.name);
        this.rerr("类型 '" + callee.obj.name + "' 没有静态方法 '" + mname + "'", e.loc);
      }
      const obj = yield* this.evalExpr(callee.obj);
      this.lastObjExpr = callee.obj;
      return yield* this.callMethod(obj, mname, args, e.loc);
    }
    if (callee.type !== "Ident") this.rerr("无法调用该表达式", e.loc);
    // 函数值变量调用（f 是 fun 类型变量）：解包函数引用并执行（含捕获环境）
    const fv = this.lookupVar(callee.name);
    if (fv && fv.alive && fv.value && fv.value.__shape === "func") {
      return yield* this.execBody(fv.value.__fn, args, e.loc, null, false, null, fv.value.__captures);
    }
    return yield* this.callFunction(callee.name, e.args, args, e.loc);
  }
  *callMethod(obj, mname, args, loc) {
    if (mname === "to_str") return valueToStr(obj);
    if (mname === "to_bytes") return this.toBytes(obj);
    if (mname === "len" && Array.isArray(obj)) return obj.length;
    if (mname === "len" && obj && obj.__shape === "slice") return obj.__len;
    if (mname === "clone" && obj && obj.__shape === "slice") {
      return obj.__arr.slice(obj.__start, obj.__start + obj.__len).map(x => deepCopyBlock(x));
    }
    // Channel 操作：经宿主（本地队列或跨线程路由）
    if (obj && obj.__shape === "channel") {
      if (mname === "send") return yield* this.host.channelSend(obj.__chan, deepCopyBlock(args[0]));
      if (mname === "recv") return yield* this.host.channelRecv(obj.__chan);
      this.rerr("Channel 没有方法 '" + mname + "'", loc);
    }
    const table = this.classMethods[obj.__type];
    const entry = table && table[mname];
    // receiver 经 sc.receiver 单独传递（execBody 的 params 不含 self）——勿把 obj 前置进参数列表
    if (entry) return yield* this.execBody(entry.func, args, loc, obj, this.objWritable(this.lastObjExpr), null);
    this.rerr("没有方法 '" + mname + "'（类型 " + obj.__type + "）", loc);
  }
  *callFunction(name, argNodes, args, loc) {
    if (name === "print") { const txt = args.map(a => typeof a === "string" ? a : valueToStr(a)).join(" "); this.host.print(txt); return { __shape: "void" }; }
    if (name === "store") { this.host.log("📦 存储 " + args[0] + " → " + valueToStr(args[1])); return { __shape: "void" }; }
    if (name === "load") { this.host.log("📂 读取 " + args[0] + "（模拟）"); return { __shape: "void" }; }
    if (name === "Channel") {
      const cap = Number(args[0]) || 1;
      const ch = new Channel(cap);
      ch.id = ++this.channelSeq;
      if (this.host.registerChannel) this.host.registerChannel(ch);
      return { __shape: "channel", __chan: ch };
    }
    const fn = this.funcs[name];
    if (!fn) this.rerr("未定义的函数 '" + name + "'", loc);
    const varRefs = argNodes.map(n => n.type === "Ident" ? n.name : null);
    return yield* this.execBody(fn, args, loc, null, false, varRefs);
  }
  *execBody(fn, argVals, loc, receiver, receiverWritable, varRefs, captures) {
    this.event("call", { name: fn.name });
    const sc = new Scope(this.current, "函数 " + fn.name);
    if (receiver) { sc.receiver = receiver; sc.receiverWritable = !!receiverWritable; }
    this.enterScope(sc);
    try {
      if (captures) {
        // 闭包捕获绑定：环境值进入函数作用域（优先于外层同名，checker 已拒参数同名）；环境私有可写
        for (const [n, v] of Object.entries(captures)) {
          sc.vars[n] = { kind: "val", value: v, mutable: true, alive: true, owned: false };
        }
      }
      fn.params.forEach((p, i) => {
        const arg = argVals[i];
        if (p.kind === "ref") {
          // ref 参数：实参是可写变量 → 绑定到变量（写透）；否则退化为值（spawn 跨执行体）
          const refName = varRefs && varRefs[i];
          if (refName) {
            const av = this.current.lookup(refName);
            if (av && av.alive && av.mutable) {
              sc.vars[p.name] = { kind: "ref", target: av, mutable: true, alive: true };
              this.event("bind_ref", { name: p.name, target: refName });
              return;
            }
          }
          sc.vars[p.name] = { kind: "val", value: arg, mutable: true, alive: true, owned: false };
          this.event("bind_ref", { name: p.name, val: valueToStr(arg) });
        } else if (p.kind === "move") {
          sc.vars[p.name] = { kind: "val", value: arg, mutable: true, alive: true, owned: true };
          this.event("move_param", { name: p.name });
        } else {
          // 切片参数：[]T 实参自动借用（[T] → 借用视图）；数组实参转切片
          let a = arg;
          if (p.ptype && p.ptype.type === "SliceType" && Array.isArray(arg)) a = { __shape: "slice", __arr: arg, __start: 0, __len: arg.length };
          const isTree = a && typeof a === "object" && a.__shape === "tree";
          const isOpt = !!(p.ptype && p.ptype.type === "OptionalType");
          if (isOpt) a = wrapOptVal(a);
          sc.vars[p.name] = { kind: "val", value: isTree ? a : deepCopyBlock(a), mutable: false, alive: true, owned: !isTree, opt: isOpt };
          this.event("bind_val", { name: p.name, val: valueToStr(a) });
        }
      });
      let result = null;
      for (const st of fn.body.stmts) {
        if (this.halted) break;
        const r = yield* this.execStmt(st);
        if (r && r.flow === "return") { result = r.value; break; }
      }
      if (result && result.__shape === "tree" && fn.ret && fn.ret.kind !== "move" && fn.ret.kind !== "ref") {
        this.rerr("树数据必须通过 move 返回（当前返回会被函数作用域销毁）", loc);
      }
      // 可选返回：裸值/null 自动包装（?T 提升）
      if (fn.ret && fn.ret.rtype && fn.ret.rtype.type === "OptionalType") result = wrapOptVal(result);
      if (result && result.__shape === "error") {
        this.event("error_propagate", { name: result.__name });
      }
      this.exitScope(sc, result);
      this.event("return", { name: fn.name, val: valueToStr(result) });
      return result;
    } catch (e) {
      this.exitScope(sc, null);
      throw e;
    }
  }

  lookupVar(name) { return this.current.lookup(name); }

  toBytes(v) {
    const clean = (x) => {
      if (x === null || x === undefined) return null;
      if (typeof x !== "object") return x;
      if (Array.isArray(x)) return x.map(clean);   // 数组元素递归（元组/块元素统一格式，对齐 C 后端）
      if (x.__shape === "void" || x.__shape === "error" || x.__shape === "enum" || x.__shape === "channel") return x;
      if (x.__shape === "optional") return { __shape: "optional", __has: x.__has, __value: x.__has ? clean(x.__value) : null };
      if (x.__kind === "tuple") {
        // 元组字节格式（ADR 0007）：位置 __items / 命名 __fields（无 __type）
        const named = x.__names.length && !/^\d+$/.test(x.__names[0]);
        if (named) {
          const f = {};
          for (let i = 0; i < x.__names.length; i++) f[x.__names[i]] = clean(x.__items[i]);
          return { __shape: "block", __fields: f };
        }
        return { __shape: "block", __items: x.__items.map(clean) };
      }
      const out = { __shape: x.__shape, __type: x.__type, __fields: {} };
      for (const [k, fv] of Object.entries(x.__fields)) out.__fields[k] = clean(fv);
      return out;
    };
    const body = clean(v);
    // 格式版本字段：顶层对象带 __ver:1（块/数组等非对象数据不带，原型）
    const payload = body && typeof body === "object" && !Array.isArray(body) ? { __ver: 1, ...body } : body;
    const json = JSON.stringify(payload);
    this.event("to_bytes", { val: valueToStr(v), bytes: json.length });
    return json;
  }
  fromBytes(s, typeName) {
    const data = JSON.parse(s);
    // 格式版本字段：顶层 __ver（缺失视为 v1，向后兼容旧数据）
    if (data && typeof data === "object" && !Array.isArray(data) && data.__ver !== undefined && data.__ver !== 1) {
      this.rerr("不支持的字节格式版本 " + data.__ver, null);
    }
    // 类型标签注册校验：__type 必须与目标类型匹配
    if (typeName && data && typeof data === "object" && !Array.isArray(data) && data.__type !== undefined && data.__type !== typeName) {
      this.rerr("字节类型标签不匹配：期望 " + typeName + "，实际 " + data.__type, null);
    }
    const revive = (x) => {
      if (x && typeof x === "object" && x.__shape) {
        if (x.__shape === "optional") {
          return { __shape: "optional", __has: !!x.__has, __value: x.__has ? revive(x.__value) : null };
        }
        if (x.__items && Array.isArray(x.__items)) {
          return { __shape: "block", __kind: "tuple", __names: x.__items.map((_, i) => String(i)), __items: x.__items.map(revive) };
        }
        if (x.__fields && x.__type === undefined) {
          const names = Object.keys(x.__fields);
          return { __shape: "block", __kind: "tuple", __names: names, __items: names.map(k => revive(x.__fields[k])) };
        }
        const fields = {};
        for (const [k, fv] of Object.entries(x.__fields)) fields[k] = revive(fv);
        return { __shape: x.__shape, __type: x.__type, __fields: fields, __alive: true };
      }
      return x;
    };
    const inst = revive(data);
    this.event("from_bytes", { val: valueToStr(inst) });
    return inst;
  }
}

function truthy(v) {
  if (v && typeof v === "object" && v.__shape === "optional") return v.__has;
  return v !== false && v !== null && v !== undefined && v !== 0 && v !== "";
}
function enumEq(l, r) {
  // 枚举值按 类型.变体 比较（与编译后端字符串常量一致）
  if (l && typeof l === "object" && l.__shape === "enum") return l.__type + "." + l.__variant === (r && typeof r === "object" ? r.__type + "." + r.__variant : String(r));
  if (r && typeof r === "object" && r.__shape === "enum") return r.__type + "." + r.__variant === (l && typeof l === "object" ? l.__type + "." + l.__variant : String(l));
  // 可选值比较：无值侧视为 null（x == null / x == 5 均可）
  const lo = l && typeof l === "object" && l.__shape === "optional";
  const ro = r && typeof r === "object" && r.__shape === "optional";
  if (lo || ro) {
    const lv = lo ? (l.__has ? l.__value : null) : l;
    const rv = ro ? (r.__has ? r.__value : null) : r;
    return lv === rv;
  }
  return l === r;
}
/* 可选包装：裸值 → optional 对象（T 自动提升 ?T）；已包装/空值 → 自身/null 包装 */
function wrapOptVal(v) {
  if (v && typeof v === "object" && v.__shape === "optional") return v;
  if (v === null || v === undefined) return { __shape: "optional", __has: false, __value: null };
  return { __shape: "optional", __has: true, __value: v };
}
/* 整数类型集合（除法整除判断）与 f32 单精度截断 */
const INT_NUM = new Set(["u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize"]);
function isIntNum(t) { return t && INT_NUM.has(t); }
function f32of(v) { return Math.fround(v); }
function binOp(op, l, r) {
  switch (op) {
    case "+": return l + r; case "-": return l - r; case "*": return l * r; case "/": return l / r;
    case "%": return l % r;
    default: return 0;
  }
}
function deepCopyBlock(v) {
  if (v === null || v === undefined || typeof v !== "object") return v;
  if (Array.isArray(v)) return v.map(deepCopyBlock);
  if (v.__shape === "tree" || v.__shape === "channel" || v.__shape === "enum" || v.__shape === "slice" || v.__shape === "func") return v;   // slice 借用视图 / func 不可变引用，不复制
  if (v.__shape === "optional") return { __shape: "optional", __has: v.__has, __value: v.__has ? deepCopyBlock(v.__value) : null };
  if (v.__kind === "tuple") return { __shape: "block", __kind: "tuple", __names: v.__names.slice(), __items: v.__items.map(deepCopyBlock) };
  if (v.__kind === "tuple") return { __shape: "block", __kind: "tuple", __names: v.__names.slice(), __items: v.__items.map(deepCopyBlock) };
  if (v.__shape === "block") {
    const out = { __shape: "block", __type: v.__type, __fields: {} };
    for (const [k, x] of Object.entries(v.__fields)) out.__fields[k] = deepCopyBlock(x);
    return out;
  }
  return JSON.parse(JSON.stringify(v));
}
function defaultValue(t) {
  if (t.type === "ArrayType") return [];
  if (t.type === "NamedType") {
    if (["u64", "f64"].includes(t.name)) return 0;
    if (t.name === "Str") return "";
    if (t.name === "bool") return false;
    if (t.name === "void") return { __shape: "void" };
    return null;
  }
  return null;
}

function run(src, ast) {
  ast = ast || parse(src);
  const ev = new Evaluator(ast);
  ev.register();
  try { ev.run(); }
  catch (e) {
    if (e.runtime) { ev.halted = true; ev.haltMsg = e.msg; ev.event("runtime_error", { msg: e.msg, line: e.line, col: e.col }); }
    else throw e;
  }
  return ev;
}

module.exports = { run, Evaluator, LocalHost, Channel, Scheduler };
