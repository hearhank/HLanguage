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

// 1. demo.h：run 成功，输出关键行
let r = h(["run", path.join(ROOT, "examples", "demo.h")]);
t("demo.h run 退出码 0", r.code === 0, "code=" + r.code);
t("demo.h 输出余额1050", r.out.includes("1050"), "");
t("demo.h 输出存储", r.out.includes("存储"), "");
t("demo.h 输出字节化往返", r.out.includes("字节化往返"), "");

// 2. error.h：错误传播，退出码 1，关键行未打印
r = h(["run", path.join(ROOT, "examples", "error.h")]);
t("error.h run 退出码 1", r.code === 1, "code=" + r.code);
t("error.h 错误信息", r.out.includes("NegativeAmount"), "");
t("error.h 错误后停止", !r.out.includes("不会执行"), "");

// 3. wrong.h：check 报 R4，run 拒绝
r = h(["check", path.join(ROOT, "examples", "wrong.h")]);
t("wrong.h check 退出码 1", r.code === 1, "code=" + r.code);
t("wrong.h check 报 R4", r.out.includes("R4"), "");
r = h(["run", path.join(ROOT, "examples", "wrong.h")]);
t("wrong.h run 拒绝执行", r.code === 1 && r.out.includes("拒绝执行"), "");

// 4. stdin
r = h(["run"], 'print("hello stdin")\n');
t("stdin 执行", r.code === 0 && r.out.includes("hello stdin"), r.out);

// 5. parse
r = h(["parse", path.join(ROOT, "examples", "demo.h")]);
t("parse 输出 AST", r.code === 0 && r.out.includes("Program"), "");

// 6. 语法错误
r = h(["run"], "fun f( {\n");
t("语法错误退出码 1", r.code === 1 && r.out.includes("语法错误"), r.out.slice(0, 60));

// 7. --trace
r = h(["run", path.join(ROOT, "examples", "demo.h"), "--trace"]);
t("--trace 输出轨迹", r.out.includes("create_var") || r.out.includes("enter_scope"), "");

// 8. match：穷尽性检查 + 求值
r = h(["run", path.join(ROOT, "examples", "match.h")]);
t("match.h 运行", r.code === 0 && r.out.includes("已支付"), r.out);

r = h(["run"], "enum S { A, B }\nfun f(s: S) -> Str {\n    return match s {\n        A => \"a\"\n    }\n}\nf(S.B)\n");
t("match 缺变体 R10", r.code === 1 && r.out.includes("未穷尽"), r.out.slice(0, 80));

r = h(["run"], "fun f(x: u64) -> u64 {\n    return match x {\n        A => 2\n    }\n}\nf(1)\n");
t("match 非枚举 R10", r.code === 1 && r.out.includes("必须是枚举"), r.out.slice(0, 80));

// 9. class import：方法提升 / hide / alias / 接口继承
r = h(["run", path.join(ROOT, "examples", "import.h")]);
t("import.h 运行", r.code === 0 && r.out.includes("账本二") && r.out.includes("150"), r.out);

r = h(["check"], "class A {\n    fun f() -> u64 { return 1 }\n}\nclass B {\n    fun f() -> u64 { return 2 }\n}\nclass C {\n    import A\n    import B\n}\n");
t("同名冲突未处理 R11", r.code === 1 && r.out.includes("同名冲突"), r.out.slice(0, 80));

r = h(["check"], "class A {\n    import B\n}\nclass B {\n    import A\n}\n");
t("导入循环 R11", r.code === 1 && r.out.includes("循环"), r.out.slice(0, 80));

r = h(["run"], "interface I {\n    fun f() -> u64\n}\nclass Base : I {\n    fun f() -> u64 { return 7 }\n}\nclass Sub {\n    import Base\n}\nfun main() {\n    s = Sub{}\n    print(s.f().to_str())\n}\nmain()\n");
t("接口继承自动标注", r.code === 0 && r.out.includes("7"), r.out);

r = h(["run"], "class B {\n    fun f() -> u64 { return 1 }\n}\nclass A {\n    import B\n    hide B::f\n}\nfun main() {\n    a = A{}\n    print(a.f().to_str())\n}\nmain()\n");
t("hide 后外部调用被拒", r.code === 1 && r.out.includes("没有方法"), r.out.slice(0, 80));

// 10. 并发：spawn + Channel 协作调度
r = h(["run", path.join(ROOT, "examples", "concurrency.h")]);
t("concurrency.h 运行", r.code === 0 && r.out.includes("消费者收到 1") && r.out.includes("消费者收到 3"), r.out);

r = h(["run"], "global ch: Channel<u64> = Channel(2)\nfun a() {\n    ch.send(1)\n    ch.send(2)\n    print(\"发送完\")\n}\nfun b() {\n    x = ch.recv()\n    print(\"收到\", x.to_str())\n}\nspawn a()\nspawn b()\n");
t("Channel 挂起/唤醒", r.code === 0 && r.out.includes("发送完") && r.out.includes("收到 1"), r.out);

r = h(["run"], "fun w() {\n    print(\"开始\")\n    yield\n    print(\"继续\")\n}\nfun x() {\n    print(\"另一执行体\")\n}\nspawn w()\nspawn x()\n");
t("yield 让出调度权", r.code === 0 && r.out.indexOf("开始") < r.out.indexOf("另一执行体") && r.out.indexOf("另一执行体") < r.out.indexOf("继续"), r.out);

// 11. 编译后端：双后端一致性
const calcPath = path.join(ROOT, "examples", "calc.h");
r = h(["run", calcPath]);
const runOut = r.out;
t("calc.h h run 运行", r.code === 0 && runOut.includes("距离平方: 25"), "");
// 编译到临时目录运行，避免污染
const tmpCalc = path.join(require("os").tmpdir(), "h_calc_test.js");
try {
  const r2 = require("child_process").spawnSync(process.execPath, [path.join(ROOT, "src", "h.js"), "build", calcPath], { encoding: "utf8" });
  const js = require("fs").readFileSync(path.join(ROOT, "calc.js"), "utf8");
  require("fs").writeFileSync(tmpCalc, js);
  const r3 = require("child_process").spawnSync(process.execPath, [tmpCalc], { encoding: "utf8" });
  t("双后端一致性：run 输出 === build 运行输出", r3.stdout === runOut, "\n--- run ---\n" + runOut + "\n--- build ---\n" + r3.stdout);
} catch (e) { t("双后端一致性", false, e.message); }

r = h(["build"], "class A {\n    fun f() -> u64 { return 1 }\n}\nfun main() -> void {\n    a = A{}\n    print(a.f().to_str())\n}\n");
t("build 拒绝 class（提示用 h run）", r.code === 1 && r.out.includes("暂不支持 class"), r.out.slice(0, 80));

// 12. M:N 并行（worker_threads）
r = h(["run", path.join(ROOT, "examples", "concurrency.h"), "--threads", "2"]);
t("M:N 并行运行 concurrency.h", r.code === 0 && r.out.includes("生产者完成") && r.out.includes("消费者收到 3"), r.out);

r = h(["run", "--threads=2"], "global ch: Channel<u64> = Channel(1)\nfun w(id: u64) {\n    print(\"执行体\", id.to_str())\n    ch.send(id)\n}\nfun reader() {\n    a = ch.recv()\n    b = ch.recv()\n    c = ch.recv()\n    print(\"读者\", a.to_str(), b.to_str(), c.to_str())\n}\nspawn w(1)\nspawn w(2)\nspawn w(3)\nspawn reader()\n");
t("跨线程 Channel 路由", r.code === 0 && r.out.includes("执行体 1") && r.out.includes("执行体 2") && r.out.includes("执行体 3") && r.out.includes("读者"), r.out);

console.log("\n" + (fail === 0 ? "✅ 全部通过 (" + pass + ")" : "❌ 失败 " + fail + " 项"));
process.exit(fail === 0 ? 0 : 1);
