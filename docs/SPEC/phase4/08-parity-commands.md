# 08 — K1–K5 对照运行指令总表（H vs Rust）

> 整理日期：2026-08-30。事实来源：`stage1/*.bat`、`stage1/README.md`、`stage2/README.md`、
> `tag1/hc-tools/tests/k1_lexer.rs` / `k2_parser.rs` / `k3_checker.rs` / `k4_interp.rs`、
> `stage1/k4test/run-compare.bat`、`stage2/test/bootstrap.bat`、本目录 `06-k5-execution-plan.md`、`07-k5-handoff.md`。

**统一对照范式**：每个阶段都是「Rust 参考命令」vs「H 版命令」，两侧 stdout 逐字节一致即 PASS，差异即 bug。

**门禁基线速览（2026-08-29）**：`k3_checker 15 ✅ / k4_interp 13 ✅ / run-compare 13 MATCH ✅ / stage2 S2 token 8/8 ✅ / S3 AST 9/9 ✅ / S8–S9 🔴`

## K1 — Lexer 词法对照（`stage1/lexer.hc`）

| 命令 | 侧 | 说明 |
|---|---|---|
| `hc lex <file.hc>` | Rust | 参考词法分析器，输出 token 流 |
| `hc run stage1/lexer.hc <file.hc>` | H | H 写的 lexer（由 Rust hc 解释执行），输出同格式 token 流 |
| `stage1\run-lexer.bat [file]` | 两侧并排 | 手动对照脚本，默认 `test_simple.hc`，先跑 H 侧再跑 Rust 侧供肉眼 diff |
| `cargo test --release -p hc-tools --test k1_lexer` | 门禁 | 自动对照：① `corpus_matches_rust_reference` 遍历 `stage1/corpus/*.hc` 逐文件比对两侧输出；② `self_source_matches` 对 lexer 自身源码对照（6621 token 零 diff） |

## K2 — Parser 语法对照（`stage1/parser.hc`）

| 命令 | 侧 | 说明 |
|---|---|---|
| `hc parse <file.hc>` | Rust | 参考解析器，输出 AST dump |
| `hc run stage1/parser.hc <file.hc>` | H | H 写的 parser，输出同格式 AST dump |
| `stage1\run-parser.bat [file]` | 两侧并排 | 手动对照脚本（默认 `test_simple.hc`） |
| `hc run stage1/interp.hc --dump-ast <file.hc>` | H | interp 内嵌 parser 的 AST 转储模式（复用 K2 dump 格式，供与 `hc parse` 互对照） |
| `cargo test --release -p hc-tools --test k2_parser` | 门禁 | ① 语料逐文件对照（01–09 纯词法语料 `hc parse` 无法解析、12-if-while 已知不支持，自动 SKIP）；② `self_source_matches`：`10-fn-basic.hc` 对照 + `hc parse` 解析 parser 自身；③ `self_hosting_parses_self`（`#[ignore]` 手动跑）：H parser 解析自身源码，与 Rust dump 一致（性能已优化到 ~1s，原 60s+） |

## K3 — Checker 语义对照（`stage1/checker.hc`）

| 命令 | 侧 | 说明 |
|---|---|---|
| `hc check <file.hc>` | Rust | 参考语义检查器（lint 警告行取最后一行比较） |
| `hc run stage1/checker.hc <file.hc>` | H | H 写的语义分析器；成功打 `OK`，失败打 `error:line:col: message` |
| `stage1\run-checker.bat [file]` | 两侧并排 | 手动对照脚本（默认 `test_simple.hc`） |
| `cargo test --release -p hc-tools --test k3_checker` | 门禁 | 15 项：语料 OK 对照（fn/var/expr/types/strings/undefined 等）；错误检测对照（`17` undefined name、`21` cannot move、`22` cannot return error literal、`23` 引用逃逸、`18` 类型错误）；`self_check_completes_on_stage1_sources`：checker 对 lexer/parser/自身三源完整跑完 0 崩溃 |

## K4 — Interp 执行对照（`stage1/interp.hc`）

| 命令 | 侧 | 说明 |
|---|---|---|
| `hc run <file.hc>` | Rust | 参考树遍历解释器直接执行，stdout 为期望输出 |
| `hc run stage1/interp.hc <file.hc>` | H | H 写的执行引擎执行同一程序，stdout 必须逐字节一致 |
| `stage1\k4test\run-compare.bat` | 一键脚本 | 遍历 `stage1/exec-corpus/01–13`，每文件两侧输出 `fc /b` 字节级 diff，汇总 MATCH/DIFF 表（快照 `k4test/compare-latest.txt`，DIFF 留存 `diff-<名>-ref/-int.txt`；exit 0 = 全 MATCH）。当前 13 MATCH |
| `cargo test --release -p hc-tools --test k4_interp` | 门禁 | 13 项 parity 测试（每语料一项：01 算术 / 02 变量 / 03 控制流 / 04 函数递归 / 05 Vec / 06 字符串 / 07 类 / 08 Map / 09 错误 / 10 综合 / 11 switch-enum / 12 cast-bits / 13 可选捕获） |

## K5 — stage2 自举编译器对照（`stage2/src/*.hc`，由 stage1 工具链驱动）

| 命令 | 侧 | 说明 |
|---|---|---|
| `hc run stage2 stage2\test\smoke.hc` | Rust | 包模式运行 stage2（Rust loader，同命名空间扁平共享 ADR-0031） |
| `hc run stage1\checker.hc stage2\src\main.hc` | H→stage2 | 用 stage1 checker 检查 stage2 全部源码（0 误报 0 漏报为 S4 验收） |
| `hc run stage1\interp.hc stage2\src\main.hc <target.hc>` | H 链路 | stage1 interp 执行 stage2 编译器（K5 主链路） |
| `hc lex <file>` ↔ `hc run stage1\interp.hc stage2\src\main.hc --dump-tokens <file>` | Rust↔H | S2 token 对照（K1 对照法）：stage2 的 `--dump-tokens` 输出 = `hc lex` 格式。当前 8/8 MATCH（含 30KB/6885 token 自身源码） |
| `hc parse <file>` ↔ `hc run stage1\interp.hc stage2\src\main.hc --dump-ast <file>` | Rust↔H | S3 AST dump 对照（K2 对照法，同格式两级 dump）。当前 9/9 MATCH（含 stage2 全部自身源码） |
| `stage2\test\bootstrap.bat` | 闭环脚本 | S8 一键自举闭环，内含三步：见下三行 |
| `hc run stage1\interp.hc stage2\src\main.hc --emit-hbc stage2\test\A.hbc <stage2 全部源文件>` | H 链路 | Phase A：interp 执行 stage2 编译器编译 stage2 自身 → `A.hbc`（嵌套解释，小时级，仅记基线） |
| `hc run stage2\test\A.hbc --emit-hbc stage2\test\B.hbc <同上>` | 产物 | Phase B：`A.hbc` 在 HBC2 VM 上再编译同源 → `B.hbc` |
| `fc /b stage2\test\A.hbc stage2\test\B.hbc` | 断言 | V1：A ≡ B 逐字节相等（二次自举可复现） |
| A.hbc 执行输出 vs Rust 编译 stage2 后执行输出 | Rust↔产物 | S9/V2 行为验证：🔴 待实现 |
