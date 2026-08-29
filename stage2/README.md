# stage2 — H 编译器（H 实现）

K5 自举闭环的本体：用 stage1 工具链编译的 H 编译器，产物可再编译产物（二次自举）。
计划与验收：`docs/SPEC/phase4/06-k5-execution-plan.md`（S1–S9/V1–V2）；交接：`07-k5-handoff.md`。

## 运行方式

```bat
:: 语义检查（stage1 checker 检查 stage2 全部源码）
hc run stage1\checker.hc stage2\main.hc

:: 运行（stage1 interp 执行 stage2 编译器；args[1] = 编译目标）
hc run stage1\interp.hc stage2\main.hc stage2\test\smoke.hc
```

## 阶段状态

| 阶段 | 文件 | 状态 | 说明 |
|---|---|---|---|
| 入口/调度 | main.hc | ✅ S1 | 读目标参数 + 阶段调度；`io.fs.read_file` 宿主透传（S1 补入 stage1 interp） |
| 词法 | lexer.hc | 🔴 S2 | 从 `stage1/interp.hc` **内嵌副本**提取（含 K5-pre 修复；非 stage1/lexer.hc，有分叉） |
| 语法 | parser.hc | 🔴 S3 | 同上（含 switch 多模式臂、import syms prop 修复） |
| 语义 | （S4 建） | 🔴 S4 | 从 checker.hc **stage2 副本**裁剪（stage1/checker.hc 冻结，K3 门禁 15 项不可破） |
| IR 模型 | （S5 建） | 🔴 S5 | IrModule/IrFunc/IrInst + kind 分发；对照 ir_inst.rs 49 变体圈定 ≤20 |
| lower | （S6 建） | 🔴 S6 | AST → IrInst |
| HBC2 编码 | （S7 建） | 🔴 S7 | 魔数 HBC2/v7；对照 encode.rs 子集；decode 丢 ret_ty/type_implements 无碍 |
| 闭环脚本 | test/bootstrap.bat | 🔴 S8 | interp 跑 stage2 → A.hbc；hc run A.hbc → B.hbc；fc /b 断言 A==B |
| 行为验证 | — | 🔴 S9 | A.hbc 执行输出 vs stage2 经 Rust 编译输出 |

## 编码纪律（违反即自举等价破坏或静默损坏）

1. **禁 Map 迭代**（确定性要求）；需要顺序时维护平行 key Vec。
2. **禁 Map 存 class 实例**（重定位损坏）；存标量索引（R2）。
3. 容器方法调用改变内容后**显式写回变量**（R5）。
4. 无闭包/接口/comptime/泛型用户代码/defer/元组（C 档绕过）。
5. **单遍编译顺序**：被依赖的自由函数先定义（R6）。
6. switch/枚举负载用 if-else 链 + kind 字符串分发（R1/R7）；switch 求值 K5-pre 已修，可选使用。
7. 对象字段写用重建 + 写回（R10）。

## checker/interp 陷阱补充（stage1 工具链实测）

- **`pi` 是 H 内置常量（π）**：循环变量勿用内置名，同名声明被静默遮蔽（`pi < n` 变 `3.14 < n` 恒假）。
- **`Map.put` 要求 `var mut` 绑定**（`Vec.append` 不要求）；跨函数改 Map 走类方法 `self` 字段。
- **局部到局部 class 拷贝**：`var a = b`（同类实例）被拒——调用点内联取值或 `copy(&x)`。
- **main 形参**：`fn main(args: Vec<String>)` 在 stage1 interp 下可取到 `args[0]`=程序自身路径、`args[1..]`=余参（S1 补入绑定；对齐 Rust hc 约定）。
- **io.fs.read_file**：宿主透传已入 stage1 interp；错误映射 NotFound/Io → 目标 Try/Catch 通道。
- **main 返回 err**：stage1 interp 会在 stdout 打印 `error: <name>`（Rust 侧为非零退出 + stderr）。

## 一次性自举链（S8 后）

```bat
hc run stage1\interp.hc stage2\main.hc stage2\main.hc   REM → A.hbc
hc run A.hbc stage2\main.hc                              REM → B.hbc
fc /b A.hbc B.hbc                                        REM V1：字节级相等
```
