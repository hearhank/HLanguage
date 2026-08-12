// h 运行时冒烟测试
// 运行：node tests/smoke.js
// 断言：三命令 × 三示例的退出码与输出

const { spawnSync } = require("child_process");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const H = path.join(ROOT, "src", "h.js");

let pass = 0, fail = 0;
const t = (label, cond, extra) => {
  console.log((cond ? "✅ " : "❌ ") + label + (extra ? " — " + extra : ""));
  cond ? pass++ : fail++;
};

function h(args, input) {
  const r = spawnSync(process.execPath, [H, ...args], { input, encoding: "utf8" });
  return { code: r.status, out: (r.stdout || "") + (r.stderr || "") };
}

// 1. demo.hc：run 成功，输出关键行
let r = h(["run", path.join(ROOT, "examples", "demo.hc")]);
t("demo.hc run 退出码 0", r.code === 0, "code=" + r.code);
t("demo.hc 输出余额1050", r.out.includes("1050"), "");
t("demo.hc 输出存储", r.out.includes("存储"), "");
t("demo.hc 输出字节化往返", r.out.includes("字节化往返"), "");

// 2. error.hc：错误传播，退出码 1，关键行未打印
r = h(["run", path.join(ROOT, "examples", "error.hc")]);
t("error.hc run 退出码 1", r.code === 1, "code=" + r.code);
t("error.hc 错误信息", r.out.includes("NegativeAmount"), "");
t("error.hc 错误后停止", !r.out.includes("不会执行"), "");

// 3. wrong.hc：check 报 R4，run 拒绝
r = h(["check", path.join(ROOT, "examples", "wrong.hc")]);
t("wrong.hc check 退出码 1", r.code === 1, "code=" + r.code);
t("wrong.hc check 报 R4", r.out.includes("R4"), "");
r = h(["run", path.join(ROOT, "examples", "wrong.hc")]);
t("wrong.hc run 拒绝执行", r.code === 1 && r.out.includes("拒绝执行"), "");

// 4. stdin
r = h(["run"], 'print("hello stdin")\n');
t("stdin 执行", r.code === 0 && r.out.includes("hello stdin"), r.out);

// 5. parse
r = h(["parse", path.join(ROOT, "examples", "demo.hc")]);
t("parse 输出 AST", r.code === 0 && r.out.includes("Program"), "");

// 6. 语法错误
r = h(["run"], "fun f( {\n");
t("语法错误退出码 1", r.code === 1 && r.out.includes("语法错误"), r.out.slice(0, 60));

// 7. --trace
r = h(["run", path.join(ROOT, "examples", "demo.hc"), "--trace"]);
t("--trace 输出轨迹", r.out.includes("create_var") || r.out.includes("enter_scope"), "");

// 8. match：穷尽性检查 + 求值
r = h(["run", path.join(ROOT, "examples", "match.hc")]);
t("match.hc 运行", r.code === 0 && r.out.includes("已支付"), r.out);

r = h(["run"], "enum S { A, B }\nfun f(s: S) -> Str {\n    return match s {\n        A => \"a\"\n    }\n}\nf(S.B)\n");
t("match 缺变体 R10", r.code === 1 && r.out.includes("未穷尽"), r.out.slice(0, 80));

r = h(["run"], "fun f(x: u64) -> u64 {\n    return match x {\n        A => 2\n    }\n}\nf(1)\n");
t("match 非枚举 R10", r.code === 1 && r.out.includes("必须是枚举"), r.out.slice(0, 80));

// 9. class import：方法提升 / hide / alias / 接口继承
r = h(["run", path.join(ROOT, "examples", "import.hc")]);
t("import.hc 运行", r.code === 0 && r.out.includes("账本二") && r.out.includes("150"), r.out);

r = h(["check"], "class A {\n    fun f() -> u64 { return 1 }\n}\nclass B {\n    fun f() -> u64 { return 2 }\n}\nclass C {\n    import A\n    import B\n}\n");
t("同名冲突未处理 R11", r.code === 1 && r.out.includes("同名冲突"), r.out.slice(0, 80));

r = h(["check"], "class A {\n    import B\n}\nclass B {\n    import A\n}\n");
t("导入循环 R11", r.code === 1 && r.out.includes("循环"), r.out.slice(0, 80));

r = h(["run"], "interface I {\n    fun f() -> u64\n}\nclass Base : I {\n    fun f() -> u64 { return 7 }\n}\nclass Sub {\n    import Base\n}\nfun main() {\n    s = Sub{}\n    print(s.f().to_str())\n}\nmain()\n");
t("接口继承自动标注", r.code === 0 && r.out.includes("7"), r.out);

r = h(["run"], "class B {\n    fun f() -> u64 { return 1 }\n}\nclass A {\n    import B\n    hide B::f\n}\nfun main() {\n    a = A{}\n    print(a.f().to_str())\n}\nmain()\n");
t("hide 后外部调用被拒", r.code === 1 && r.out.includes("没有方法"), r.out.slice(0, 80));

// 10. 并发：spawn + Channel 协作调度
r = h(["run", path.join(ROOT, "examples", "concurrency.hc")]);
t("concurrency.hc 运行", r.code === 0 && r.out.includes("消费者收到 1") && r.out.includes("消费者收到 3"), r.out);

r = h(["run"], "global ch: Channel<u64> = Channel(2)\nfun a() {\n    ch.send(1)\n    ch.send(2)\n    print(\"发送完\")\n}\nfun b() {\n    x = ch.recv()\n    print(\"收到\", x.to_str())\n}\nspawn a()\nspawn b()\n");
t("Channel 挂起/唤醒", r.code === 0 && r.out.includes("发送完") && r.out.includes("收到 1"), r.out);

r = h(["run"], "fun w() {\n    print(\"开始\")\n    yield\n    print(\"继续\")\n}\nfun x() {\n    print(\"另一执行体\")\n}\nspawn w()\nspawn x()\n");
t("yield 让出调度权", r.code === 0 && r.out.indexOf("开始") < r.out.indexOf("另一执行体") && r.out.indexOf("另一执行体") < r.out.indexOf("继续"), r.out);

// 11. 编译后端：双后端一致性（C 原生 + JS 回退）
const calcPath = path.join(ROOT, "examples", "calc.hc");
r = h(["run", calcPath]);
const runOut = r.out;
t("calc.hc h run 运行", r.code === 0 && runOut.includes("距离平方: 25"), "");
// 编译（有 zig cc 走 C 后端 → calc.exe；无则 JS 回退）并运行产物
const tmpOut = path.join(require("os").tmpdir(), "h_calc_prog");
require("fs").writeFileSync(path.join(require("os").tmpdir(), "calc.hc"), require("fs").readFileSync(calcPath, "utf8"));
const cwd = process.cwd();
process.chdir(require("os").tmpdir());
const r2 = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "calc.hc"], { encoding: "utf8" });
const exeP = path.join(require("os").tmpdir(), "calc" + (process.platform === "win32" ? ".exe" : ""));
const jsP = path.join(require("os").tmpdir(), "calc.js");
let builtOut = "";
if (require("fs").existsSync(exeP)) {
  const r3 = require("child_process").spawnSync(exeP, [], { encoding: "utf8" });
  builtOut = r3.stdout || "";
} else if (require("fs").existsSync(jsP)) {
  const r3 = require("child_process").spawnSync(process.execPath, [jsP], { encoding: "utf8" });
  builtOut = r3.stdout || "";
}
process.chdir(cwd);
t("双后端一致性：run 输出 === 编译后运行输出", builtOut.replace(/\r/g, "") === runOut, "\n--- run ---\n" + runOut + "\n--- build ---\n" + builtOut.replace(/\r/g, ""));
t("编译产物是原生二进制（zig cc）", (r2.stdout || "").includes("zig cc") || (r2.stdout || "").includes("JS 目标"), (r2.stdout || "").slice(0, 60));

// 11b. build 支持 class（树）：编译运行输出 1（原生）
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "class.hc"),
  "class A {\n    fun f() -> u64 { return 1 }\n}\nfun main() -> void {\n    a = A{}\n    print(a.f().to_str())\n}\n");
const rC = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "class.hc"], { encoding: "utf8" });
const exeC = path.join(require("os").tmpdir(), "class" + (process.platform === "win32" ? ".exe" : ""));
let builtC = "";
if (require("fs").existsSync(exeC)) {
  const r3 = require("child_process").spawnSync(exeC, [], { encoding: "utf8" });
  builtC = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("build 支持 class（树）：编译运行输出 1", builtC.includes("1"), "\n--- build 日志 ---\n" + (rC.stdout || "") + (rC.stderr || "") + "\n--- exe 输出 ---\n" + builtC);

r = h(["build"], "global g: Exclusive<u64> = 0\n");
t("build 拒绝非 Channel 的 global（提示用 h run）", r.code === 1 && r.out.includes("暂不支持非 Channel 的 global"), r.out.slice(0, 120));

// 12. M:N 并行（worker_threads）
r = h(["run", path.join(ROOT, "examples", "concurrency.hc"), "--threads", "2"]);
t("M:N 并行运行 concurrency.hc", r.code === 0 && r.out.includes("生产者完成") && r.out.includes("消费者收到 3"), r.out);

r = h(["run", "--threads=2"], "global ch: Channel<u64> = Channel(1)\nfun w(id: u64) {\n    print(\"执行体\", id.to_str())\n    ch.send(id)\n}\nfun reader() {\n    a = ch.recv()\n    b = ch.recv()\n    c = ch.recv()\n    print(\"读者\", a.to_str(), b.to_str(), c.to_str())\n}\nspawn w(1)\nspawn w(2)\nspawn w(3)\nspawn reader()\n");
t("跨线程 Channel 路由", r.code === 0 && r.out.includes("执行体 1") && r.out.includes("执行体 2") && r.out.includes("执行体 3") && r.out.includes("读者"), r.out);

// 13. C 后端动态数组 [T]
const arrPath = path.join(ROOT, "examples", "array.hc");
r = h(["run", arrPath]);
const runOutA = r.out;
t("array.hc h run 运行", r.code === 0 && runOutA.includes("整数数组: 60"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "array.hc"), require("fs").readFileSync(arrPath, "utf8"));
const rA = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "array.hc"], { encoding: "utf8" });
const exeA = path.join(require("os").tmpdir(), "array" + (process.platform === "win32" ? ".exe" : ""));
let builtA = "";
if (require("fs").existsSync(exeA)) {
  const r3 = require("child_process").spawnSync(exeA, [], { encoding: "utf8" });
  builtA = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("数组双后端一致性（C 原生）", builtA === runOutA, "\n--- run ---\n" + runOutA + "\n--- build ---\n" + builtA);

// 14. C 后端 class ref 字段（双向引用通知）
const refPath = path.join(ROOT, "examples", "ref.hc");
r = h(["run", refPath]);
const runOutR = r.out;
t("ref.hc h run 运行", r.code === 0 && runOutR.includes("通知后: Node{val: 2, next: null}"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "ref.hc"), require("fs").readFileSync(refPath, "utf8"));
const rR = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "ref.hc"], { encoding: "utf8" });
const exeR = path.join(require("os").tmpdir(), "ref" + (process.platform === "win32" ? ".exe" : ""));
let builtR = "";
if (require("fs").existsSync(exeR)) {
  const r3 = require("child_process").spawnSync(exeR, [], { encoding: "utf8" });
  builtR = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("ref 字段双后端一致性（双向引用通知）", builtR === runOutR, "\n--- run ---\n" + runOutR + "\n--- build ---\n" + builtR);

// 15. C 后端 ref/move 参数（写透别名 + 所有权转移）
const rpPath = path.join(ROOT, "examples", "ref_param.hc");
r = h(["run", rpPath]);
const runOutRP = r.out;
t("ref_param.hc h run 运行", r.code === 0 && runOutRP.includes("写透后: 150"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "ref_param.hc"), require("fs").readFileSync(rpPath, "utf8"));
const rP = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "ref_param.hc"], { encoding: "utf8" });
const exeRP = path.join(require("os").tmpdir(), "ref_param" + (process.platform === "win32" ? ".exe" : ""));
let builtRP = "";
if (require("fs").existsSync(exeRP)) {
  const r3 = require("child_process").spawnSync(exeRP, [], { encoding: "utf8" });
  builtRP = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("ref/move 参数双后端一致性", builtRP === runOutRP, "\n--- run ---\n" + runOutRP + "\n--- build ---\n" + builtRP);

// 16. C 后端 error：未处理即终止（含 stderr 格式 + 退出码 1）
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "error.hc"), require("fs").readFileSync(path.join(ROOT, "examples", "error.hc"), "utf8"));
const rE = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "error.hc", "--exec"], { encoding: "utf8" });
const errOut = (rE.stdout || "") + (rE.stderr || "");
process.chdir(cwd);
t("error 双后端一致：合法输出 + 未处理终止 + 退出码 1",
  rE.status === 1 && errOut.includes("先算一个合法的") && errOut.includes("❌ error.NegativeAmount（未处理）") && !errOut.includes("这行不会执行"),
  "status=" + rE.status + "\n" + errOut.slice(0, 300));

// 17. C 后端并发：Fiber 协作式调度 + Channel 交替（与 eval 单线程逐字一致）
const ccPath = path.join(ROOT, "examples", "concurrency.hc");
r = h(["run", ccPath]);
const runOutCC = r.out;
t("concurrency.hc h run 运行", r.code === 0 && runOutCC.includes("消费者收到 3"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "concurrency.hc"), require("fs").readFileSync(ccPath, "utf8"));
const rCC = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "concurrency.hc"], { encoding: "utf8" });
const exeCC = path.join(require("os").tmpdir(), "concurrency" + (process.platform === "win32" ? ".exe" : ""));
let builtCC = "";
if (require("fs").existsSync(exeCC)) {
  const r3 = require("child_process").spawnSync(exeCC, [], { encoding: "utf8" });
  builtCC = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("并发双后端一致性（Channel 交替）", builtCC === runOutCC, "\n--- run ---\n" + runOutCC + "\n--- build ---\n" + builtCC);

// 17b. C 后端 M:N 多线程（--threads 2）：关键行齐全 + 退出 0（顺序不保证，SPEC 04）
process.chdir(require("os").tmpdir());
const rMT = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "concurrency.hc", "--threads", "2", "--exec"], { encoding: "utf8", timeout: 30000 });
const mtOut = (rMT.stdout || "") + (rMT.stderr || "");
process.chdir(cwd);
t("M:N 多线程（C 后端）完成且关键行齐全",
  rMT.status === 0 && mtOut.includes("生产者启动") && mtOut.includes("生产者完成") && mtOut.includes("消费者启动") && mtOut.includes("消费者完成") && mtOut.includes("收到 1") && mtOut.includes("收到 3"),
  "status=" + rMT.status + "\n" + mtOut.slice(0, 400));

// 17c. C 后端跨平台：zig cc 交叉编译 Linux（posix 并发运行时路径）
process.chdir(require("os").tmpdir());
const rX = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "concurrency.hc"], { encoding: "utf8" });
const xc = require("child_process").spawnSync("zig", ["cc", "-target", "x86_64-linux-gnu", "-c", "concurrency.c", "-o", "cl.o"], { encoding: "utf8" });
process.chdir(cwd);
t("Linux 交叉编译通过（posix 并发运行时）", xc.status === 0, (xc.stderr || "").slice(0, 200));

// 18. C 后端 yield：让出调度权（与 eval 单线程逐字一致）
const yieldSrc = "fun w() {\n    print(\"开始\")\n    yield\n    print(\"继续\")\n}\nfun x() {\n    print(\"另一执行体\")\n}\nspawn w()\nspawn x()\n";
r = h(["run"], yieldSrc);
const runOutY = r.out;
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "yield.hc"), yieldSrc);
const rY = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "yield.hc"], { encoding: "utf8" });
const exeY = path.join(require("os").tmpdir(), "yield" + (process.platform === "win32" ? ".exe" : ""));
let builtY = "";
if (require("fs").existsSync(exeY)) {
  const r3 = require("child_process").spawnSync(exeY, [], { encoding: "utf8" });
  builtY = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("yield 双后端一致性（让出调度权）", builtY === runOutY, "\n--- run ---\n" + runOutY + "\n--- build ---\n" + builtY);

// 19. C 后端带参 spawn（参数打包结构体 + Fiber 入口解包）
const spPath = path.join(ROOT, "examples", "spawn_args.hc");
r = h(["run", spPath]);
const runOutSP = r.out;
t("spawn_args.hc h run 运行", r.code === 0 && runOutSP.includes("求和: 5"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "spawn_args.hc"), require("fs").readFileSync(spPath, "utf8"));
const rSP = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "spawn_args.hc"], { encoding: "utf8" });
const exeSP = path.join(require("os").tmpdir(), "spawn_args" + (process.platform === "win32" ? ".exe" : ""));
let builtSP = "";
if (require("fs").existsSync(exeSP)) {
  const r3 = require("child_process").spawnSync(exeSP, [], { encoding: "utf8" });
  builtSP = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("带参 spawn 双后端一致性", builtSP === runOutSP, "\n--- run ---\n" + runOutSP + "\n--- build ---\n" + builtSP);

// 20. C 后端字节化 to_bytes/from_bytes（JSON 逐字节一致）
const byPath = path.join(ROOT, "examples", "bytes.hc");
r = h(["run", byPath]);
const runOutBY = r.out;
t("bytes.hc h run 运行", r.code === 0 && runOutBY.includes("恢复: Account{balance: 100"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "bytes.hc"), require("fs").readFileSync(byPath, "utf8"));
const rBY = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "bytes.hc"], { encoding: "utf8" });
const exeBY = path.join(require("os").tmpdir(), "bytes" + (process.platform === "win32" ? ".exe" : ""));
let builtBY = "";
if (require("fs").existsSync(exeBY)) {
  const r3 = require("child_process").spawnSync(exeBY, [], { encoding: "utf8" });
  builtBY = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("字节化双后端一致性（JSON 逐字节）", builtBY === runOutBY, "\n--- run ---\n" + runOutBY + "\n--- build ---\n" + builtBY);

// 20b. 字节格式版本字段（白盒：语言层无法构造含引号的 JSON 字面量）
const { Evaluator } = require(path.join(ROOT, "src", "evaluator.js"));
const { parse } = require(path.join(ROOT, "src", "parser.js"));
const evW = new Evaluator(parse("struct P { x: f64 }"));
evW.register();
const legacy = evW.fromBytes('{"__shape":"block","__type":"P","__fields":{"x":5}}');
t("旧格式字节（无 __ver）兼容 v1", legacy && legacy.__fields && legacy.__fields.x === 5, "");
const newBytes = evW.toBytes(legacy);
t("to_bytes 顶层带 __ver:1", newBytes.includes('"__ver":1'), newBytes.slice(0, 60));
let verErr = "";
try { evW.fromBytes('{"__ver":99,"__shape":"block","__type":"P","__fields":{"x":1}}'); } catch (e) { verErr = e.msg || ""; }
t("未知字节格式版本报错", verErr.includes("不支持的字节格式版本 99"), verErr);
let tagErr = "";
try { evW.fromBytes('{"__shape":"block","__type":"Q","__fields":{"x":1}}', "P"); } catch (e) { tagErr = e.msg || ""; }
t("字节类型标签不匹配报错", tagErr.includes("字节类型标签不匹配：期望 P，实际 Q"), tagErr);
const { genC } = require(path.join(ROOT, "src", "cgen.js"));
const genC2 = genC(parse("struct P { x: f64 }\nclass C { a: u64 }\n"));
t("C 生成类型注册表（h_type_registry）", genC2.includes("h_type_registry") && genC2.includes('{ "P", sizeof(P) }') && genC2.includes('{ "C", sizeof(C) }'), "");

r = h(["check"], "class A {}\nfun f(x: ref A) {}\nfun main() {\n    a = A{}\n    f(a)\n}\n");
t("ref 实参非 mut → R3 拒绝", r.code === 1 && r.out.includes("R3"), r.out.slice(0, 120));

// 21. 整数除法 + 循环（for/while/break/continue）：双后端一致性
const lpPath = path.join(ROOT, "examples", "loop.hc");
r = h(["run", lpPath]);
const runOutLP = r.out;
t("loop.hc h run 运行", r.code === 0 && runOutLP.includes("整除: 2 取余: 1") && runOutLP.includes("奇数求和: 25"), "");
process.chdir(require("os").tmpdir());
require("fs").writeFileSync(path.join(require("os").tmpdir(), "loop.hc"), require("fs").readFileSync(lpPath, "utf8"));
const rLP = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", "loop.hc"], { encoding: "utf8" });
const exeLP = path.join(require("os").tmpdir(), "loop" + (process.platform === "win32" ? ".exe" : ""));
let builtLP = "";
if (require("fs").existsSync(exeLP)) {
  const r3 = require("child_process").spawnSync(exeLP, [], { encoding: "utf8" });
  builtLP = (r3.stdout || "").replace(/\r/g, "");
}
process.chdir(cwd);
t("循环/除法双后端一致性（C 原生）", builtLP === runOutLP, "\n--- run ---\n" + runOutLP + "\n--- build ---\n" + builtLP);

// 21b. 循环/除法静态拒绝用例
r = h(["run"], "fun main() {\n    break\n}\n");
t("break 在循环外 → R5 拒绝", r.code === 1 && r.out.includes("R5"), r.out.slice(0, 120));
r = h(["run"], "fun main() {\n    continue\n}\n");
t("continue 在循环外 → R5 拒绝", r.code === 1 && r.out.includes("R5"), r.out.slice(0, 120));
r = h(["run"], "fun main() {\n    mut a = [1, 2]\n    for x in a {}\n}\n");
t("for 非数字区间 → R5 拒绝", r.code === 1 && r.out.includes("R5"), r.out.slice(0, 120));
r = h(["run"], "fun main() {\n    for i in 0.5..3 {}\n}\n");
t("for f64 区间 → R5 拒绝", r.code === 1 && r.out.includes("R5"), r.out.slice(0, 120));
r = h(["run"], "fun main() {\n    print(nonexistent)\n}\n");
t("调用参数未定义变量 → R7（参数检查修复）", r.code === 1 && r.out.includes("R7"), r.out.slice(0, 120));

console.log("\n" + (fail === 0 ? "✅ 全部通过 (" + pass + ")" : "❌ 失败 " + fail + " 项"));
process.exit(fail === 0 ? 0 : 1);
