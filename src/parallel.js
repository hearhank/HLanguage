// H 语言 M:N 并行协调器——主线程：Channel 路由 + 输出聚合 + 执行体分派
// worker 线程：执行体宿主（各自协作式队列）
// 多线程模式下 print 输出顺序不保证（与单线程模式可能不同）

const { Worker } = require("worker_threads");
const path = require("path");
const { check } = require("./checker");
const { Evaluator, Scheduler } = require("./evaluator");

function encodeForWorker(v) {
  if (v && typeof v === "object" && v.__shape === "channel") {
    return { __shape: "channel", __chanId: v.__chan.id, cap: v.__chan.cap };
  }
  if (v && typeof v === "object" && v.__shape && v.__fields) {
    const out = { __shape: v.__shape, __type: v.__type, __fields: {}, __alive: true };
    for (const [k, x] of Object.entries(v.__fields)) out.__fields[k] = encodeForWorker(x);
    return out;
  }
  if (Array.isArray(v)) return v.map(encodeForWorker);
  return v;
}

/* 主线程 main 执行体的宿主 */
class MainHost {
  constructor(runner) { this.runner = runner; this.ev = null; }
  print(text) { this.runner.onOutput(text); }
  log(text) { this.runner.onOutput(text); }
  registerChannel(ch) {
    this.runner.channels.set(ch.id, { buf: [], cap: ch.cap, waiters: [] });
  }
  *channelSend(ch, value) {
    const st = this.runner.channels.get(ch.id);
    if (st.buf.length < st.cap) {
      st.buf.push(value);
      this.runner.wakeRecv(st);
      return true;
    }
    const taskId = ++this.ev.channelSeq * 100000 + 1;
    st.waiters.push({ type: "send", taskId, value, isMain: true });
    return yield { k: "wait_ext", id: taskId };
  }
  *channelRecv(ch) {
    const st = this.runner.channels.get(ch.id);
    if (st.buf.length) {
      const v = st.buf.shift();
      this.runner.wakeSend(st);
      return v;
    }
    const taskId = ++this.ev.channelSeq * 100000 + 2;
    st.waiters.push({ type: "recv", taskId, isMain: true });
    return yield { k: "wait_ext", id: taskId };
  }
  *spawn(fn, argVals, loc) {
    const w = this.runner.workers[this.runner.spawnRound++ % this.runner.workers.length];
    this.runner.active++;
    // 附带 global channel 绑定（worker 需以主线程分配的 channel id 访问全局 ch）
    const bindings = [];
    for (const [name, v] of Object.entries(this.ev.global.vars)) {
      if (v.value && v.value.__shape === "channel") {
        bindings.push({ name, chanId: v.value.__chan.id, cap: v.value.__chan.cap });
      }
    }
    w.postMessage({ type: "spawn", fn: fn.name, argVals: encodeForWorker(argVals), globals: bindings });
    return { __shape: "void" };
  }
}

class ParallelRunner {
  constructor(source, threads, opts) {
    this.source = source;
    this.threads = Math.max(1, threads | 0);
    this.onOutput = opts.onOutput;
    this.onDone = opts.onDone;
    this.onError = opts.onError;
    this.channels = new Map();
    this.workers = [];
    this.spawnRound = 0;
    this.active = 0;
    this.mainDone = false;
    this.finished = false;
  }

  start() {
    for (let i = 0; i < this.threads; i++) {
      const w = new Worker(path.join(__dirname, "worker_host.js"));
      w.on("message", (msg) => this.onWorkerMsg(w, msg));
      w.on("error", (e) => this.onError("worker 错误: " + e.message));
      w.postMessage({ type: "init", source: this.source });
      this.workers.push(w);
    }
    const ast = check(this.source).ast;   // 复用静态检查的 AST（含除法等类型标注）
    const mainEv = new Evaluator(ast, new MainHost(this));
    this.mainEv = mainEv;
    this.mainHost = mainEv.host;
    this.mainHost.ev = mainEv;
    mainEv.register();
    const origEvent = mainEv.event.bind(mainEv);
    mainEv.event = (t, data) => {
      if (t === "main_done") this.mainDone = true;
      origEvent(t, data);
    };
    this.mainSched = new Scheduler(mainEv);
    mainEv.sched = this.mainSched;
    this.mainGen = mainEv.execTopLevel();
    this.mainSched.push(this.mainGen, "main", mainEv.current);
    this.advanceMain();
  }

  advanceMain() {
    this.mainSched.run();
    if (this.mainDone) this.checkDone();
  }

  wakeRecv(st) {
    const i = st.waiters.findIndex(w => w.type === "recv");
    if (i >= 0 && st.buf.length) {
      const w = st.waiters.splice(i, 1)[0];
      const v = st.buf.shift();
      if (w.isMain) { this.mainSched.wakeExt(w.taskId, v); this.advanceMain(); }
      else w.worker.postMessage({ type: "wake", taskId: w.taskId, result: v });
      this.wakeSend(st);
    }
  }
  wakeSend(st) {
    const i = st.waiters.findIndex(w => w.type === "send");
    if (i >= 0 && st.buf.length < st.cap) {
      const w = st.waiters.splice(i, 1)[0];
      st.buf.push(w.value);
      if (w.isMain) { this.mainSched.wakeExt(w.taskId, true); this.advanceMain(); }
      else w.worker.postMessage({ type: "wake", taskId: w.taskId, result: true });
      this.wakeRecv(st);
    }
  }

  onWorkerMsg(w, msg) {
    if (this.finished) return;
    if (msg.type === "output") {
      this.onOutput(msg.text);
    } else if (msg.type === "chan") {
      const st = this.channels.get(msg.id);
      if (!st) { this.onError("未知 channel " + msg.id); return; }
      if (msg.op === "send") {
        if (st.buf.length < st.cap) {
          st.buf.push(msg.value);
          w.postMessage({ type: "wake", taskId: msg.taskId, result: true });
          this.wakeRecv(st);
        } else {
          st.waiters.push({ type: "send", taskId: msg.taskId, value: msg.value, worker: w });
        }
      } else {
        if (st.buf.length) {
          const v = st.buf.shift();
          w.postMessage({ type: "wake", taskId: msg.taskId, result: v });
          this.wakeSend(st);
        } else {
          st.waiters.push({ type: "recv", taskId: msg.taskId, worker: w });
        }
      }
    } else if (msg.type === "done") {
      this.active = Math.max(0, this.active - (msg.count || 1));
      this.checkDone();
    } else if (msg.type === "error") {
      this.onError(msg.msg);
    }
  }

  checkDone() {
    if (this.finished) return;
    if (this.mainDone && this.active <= 0) {
      this.finished = true;
      for (const w of this.workers) w.terminate();
      this.onDone();
    }
  }
}

module.exports = { ParallelRunner };
