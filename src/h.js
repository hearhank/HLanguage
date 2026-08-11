#!/usr/bin/env node
// h —— H 语言运行时雏形（CLI）
// 用法：
//   h run <file>         解析 → 静态检查 → 求值（打印 print/store 输出）
//   h check <file>       只做静态检查（R1-R9）
//   h parse <file>       打印 AST（JSON）
//   h run                无文件时从 stdin 读取源码
//   --trace              附带执行轨迹（事件流，输出到 stderr）

const fs = require("fs");
const path = require("path");
const os = require("os");
const { spawnSync } = require("child_process");
const { lex } = require("./lexer");
const { parse } = require("./parser");
const { check } = require("./checker");
const { run } = require("./evaluator");
const { jsgen } = require("./jsgen");
const { ParallelRunner } = require("./parallel");

const HELP = `h —— H 语言运行时雏形

用法:
  h run <file>         解析 → 静态检查 → 求值（打印输出）
  h check <file>       只做静态检查（R1-R11）
  h parse <file>       打印 AST（JSON）
  h build <file>       编译为可执行 JS（纯块子集；--exec 编译后直接运行）
  h run                从 stdin 读取源码执行

选项:
  --trace              输出执行轨迹（事件流到 stderr）
  --exec               编译后立即运行产物
  -h, --help           显示帮助
`;

function fmtErr(e) {
  if (e.lex || e.parse) return "语法错误 @第" + e.line + "行:" + e.col + " 列 —— " + e.msg;
  if (e.runtime) return "运行时错误 @第" + e.line + "行:" + e.col + " 列 —— " + e.msg;
  return "内部错误: " + (e.stack || e.message);
}

function loadSource(file) {
  if (file) {
    if (!fs.existsSync(file)) { console.error("h: 找不到文件 '" + file + "'"); process.exit(1); }
    return fs.readFileSync(file, "utf8");
  }
  return fs.readFileSync(0, "utf8");   // stdin
}

function doParse(src) {
  const ast = parse(src);
  console.log(JSON.stringify(ast, null, 2));
}

function doCheck(src) {
  const r = check(src);
  if (r.errors.length === 0) {
    console.log("✅ 静态检查通过：类型 " + Object.keys(r.types).length + " 个，函数 " + Object.keys(r.funcs).length + " 个");
    return 0;
  }
  for (const e of r.errors) {
    console.error("[" + e.rule + "] 第" + e.line + "行:" + e.col + " 列 —— " + e.msg);
  }
  console.error("❌ 发现 " + r.errors.length + " 个静态错误");
  return 1;
}

function doRun(src, trace) {
  // 静态检查先行
  const r = check(src);
  if (r.errors.length > 0) {
    for (const e of r.errors) console.error("[" + e.rule + "] 第" + e.line + "行:" + e.col + " 列 —— " + e.msg);
    console.error("❌ " + r.errors.length + " 个静态错误，拒绝执行");
    return 1;
  }
  // 求值
  const ev = run(src);
  // 输出（print/store/load）
  for (const line of ev.output) console.log(line);
  // 轨迹
  if (trace) {
    for (const e of ev.events) {
      const s = JSON.stringify(e.snap || null);
      console.error("[" + e.t + "]" + (e.name ? " " + e.name : "") + (e.val !== undefined ? " " + e.val : "") + (e.msg ? " " + e.msg : ""));
    }
  }
  if (ev.halted) {
    console.error("❌ " + ev.haltMsg);
    return 1;
  }
  return 0;
}

function doBuild(src, file, execIt) {
  // 静态检查先行
  const r = check(src);
  if (r.errors.length > 0) {
    for (const e of r.errors) console.error("[" + e.rule + "] 第" + e.line + "行:" + e.col + " 列 —— " + e.msg);
    console.error("❌ " + r.errors.length + " 个静态错误，拒绝编译");
    return 1;
  }
  let js;
  try { js = jsgen(parse(src)); }
  catch (e) {
    console.error("❌ 编译失败：" + e.message);
    return 1;
  }
  const base = file ? path.basename(file, path.extname(file)) : "h_prog";
  const outFile = path.join(process.cwd(), base + ".js");
  fs.writeFileSync(outFile, js);
  console.log("✅ 编译成功 → " + outFile);
  if (execIt) {
    const r2 = spawnSync(process.execPath, [outFile], { encoding: "utf8" });
    process.stdout.write(r2.stdout || "");
    if (r2.stderr) process.stderr.write(r2.stderr);
    return r2.status || 0;
  }
  return 0;
}

function doRunParallel(src, threads) {
  // 静态检查先行
  const r = check(src);
  if (r.errors.length > 0) {
    for (const e of r.errors) console.error("[" + e.rule + "] 第" + e.line + "行:" + e.col + " 列 —— " + e.msg);
    console.error("❌ " + r.errors.length + " 个静态错误，拒绝执行");
    return 1;
  }
  const runner = new ParallelRunner(src, threads, {
    onOutput: (text) => console.log(text),
    onDone: () => { process.exit(0); },
    onError: (msg) => { console.error("❌ " + msg); process.exit(1); },
  });
  runner.start();
  return 0;   // 不退出——等待回调
}

function main() {
  const args = process.argv.slice(2);
  let trace = false, execIt = false, threads = 0;
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--trace") trace = true;
    else if (a === "--exec") execIt = true;
    else if (a === "--threads" && i + 1 < args.length) threads = Number(args[++i]) || 0;
    else if (a.startsWith("--threads=")) threads = Number(a.slice("--threads=".length)) || 0;
    else if (a === "-h" || a === "--help") { console.log(HELP); return; }
    else rest.push(a);
  }
  const cmd = rest[0] || "run";
  const file = rest[1];
  let src;
  try { src = loadSource(file); }
  catch (e) { console.error("h: " + e.message); process.exit(1); }
  try {
    let code = 0;
    if (cmd === "run" && threads > 0) { doRunParallel(src, threads); return; }  // 回调退出
    if (cmd === "run") code = doRun(src, trace);
    else if (cmd === "check") code = doCheck(src);
    else if (cmd === "parse") doParse(src);
    else if (cmd === "build") code = doBuild(src, file, execIt);
    else { console.error("h: 未知命令 '" + cmd + "'"); console.log(HELP); process.exit(1); }
    process.exit(code);
  } catch (e) {
    console.error(fmtErr(e));
    process.exit(1);
  }
}

main();
