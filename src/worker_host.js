// H 语言 worker 线程宿主——执行体在独立线程运行
// 与主线程通过 postMessage 通信：print 输出、Channel 路由、执行体完成
// Channel 操作经主线程单点路由（无竞争）；数据经结构化克隆（跨线程深拷贝）

const { parentPort } = require("worker_threads");
const { parse } = require("./parser");
const { Evaluator, Scheduler } = require("./evaluator");

class WorkerHost {
  constructor(ev) {
    this.ev = ev;
    this.taskSeq = 0;
  }
  print(text) { parentPort.postMessage({ type: "output", text }); }
  log(text) { parentPort.postMessage({ type: "output", text }); }
  registerChannel() {}
  *channelSend(ch, value) {
    // 主线程路由：发消息并挂起等待结果
    const taskId = ++this.taskSeq;
    parentPort.postMessage({ type: "chan", op: "send", id: ch.id, taskId, value });
    return yield { k: "wait_ext", id: taskId };
  }
  *channelRecv(ch) {
    const taskId = ++this.taskSeq;
    parentPort.postMessage({ type: "chan", op: "recv", id: ch.id, taskId });
    return yield { k: "wait_ext", id: taskId };
  }
  *spawn(fn, argVals, loc) {
    // worker 内嵌套 spawn：本地执行
    const gen = this.ev.execBody(fn, argVals, loc, null, false);
    this.ev.sched.push(gen, fn.name, this.ev.current);
    return { __shape: "void" };
  }
}

let ev = null;
let globalsInited = false;

function initGlobals(bindings) {
  if (globalsInited || !ev) return;
  globalsInited = true;
  for (const d of ev.ast.decls) {
    if (d.type !== "GlobalDecl") continue;
    let init = null;
    if (d.init && d.init.type === "CallExpr" && d.init.callee.type === "Ident" && d.init.callee.name === "Channel") {
      const b = bindings && bindings.find(x => x.name === d.name);
      init = { __shape: "channel", __chan: { id: b ? b.chanId : 0, cap: b ? b.cap : 1 } };
    } else if (d.init && d.init.type === "ArrayLiteral") {
      init = [];
    }
    ev.global.vars[d.name] = { kind: "val", value: init, mutable: true, alive: true, owned: true };
  }
}

function decodeArg(v) {
  // 主线程编码的 channel 占位 → worker 本地 channel
  if (v && typeof v === "object" && v.__shape === "channel") {
    return { __shape: "channel", __chan: { id: v.__chanId, cap: v.cap || 1 } };
  }
  if (v && typeof v === "object" && v.__fields) {
    const out = { __shape: v.__shape, __type: v.__type, __fields: {}, __alive: true };
    for (const [k, x] of Object.entries(v.__fields)) out.__fields[k] = decodeArg(x);
    return out;
  }
  if (Array.isArray(v)) return v.map(decodeArg);
  return v;
}

function runLocalQueue() {
  if (!ev) return;
  try {
    ev.sched.run();
  } catch (e) {
    parentPort.postMessage({ type: "error", msg: (e && (e.msg || e.message)) || String(e), line: e && e.line, col: e && e.col });
    return;
  }
  if (ev.completedSinceLast) {
    parentPort.postMessage({ type: "done", count: ev.completedSinceLast });
    ev.completedSinceLast = 0;
  }
}

parentPort.on("message", (msg) => {
  if (msg.type === "init") {
    const ast = parse(msg.source);
    ev = new Evaluator(ast, null);
    ev.host = new WorkerHost(ev);
    ev.sched = new Scheduler(ev);
    ev.completedSinceLast = 0;
    ev.register();
    // 拦截 exec_done 计数：包装 event
    const origEvent = ev.event.bind(ev);
    ev.event = (t, data) => {
      if (t === "exec_done") ev.completedSinceLast++;
      origEvent(t, data);
    };
  } else if (msg.type === "spawn") {
    initGlobals(msg.globals);
    const fn = ev.funcs[msg.fn];
    if (!fn) { parentPort.postMessage({ type: "error", msg: "worker: 未定义函数 " + msg.fn }); return; }
    const argVals = (msg.argVals || []).map(decodeArg);
    const gen = ev.execBody(fn, argVals, null, null, false);
    ev.sched.push(gen, fn.name, ev.global);
    runLocalQueue();
  } else if (msg.type === "wake") {
    ev.sched.wakeExt(msg.taskId, msg.result);
    runLocalQueue();
  }
});
