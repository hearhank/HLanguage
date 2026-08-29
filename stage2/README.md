# stage2 — H 编译器（H 实现）

K5 自举闭环的本体：用 stage1 工具链编译的 H 编译器，产物可再编译产物（二次自举）。
标准 hc 项目结构：`build.zon` + `src/`（同包共享命名空间）+ `test/`（冒烟目标与闭环脚本）。
计划与验收：`docs/SPEC/phase4/06-k5-execution-plan.md`（S1–S9/V1–V2）；交接：`07-k5-handoff.md`。

## 运行方式

```bat
:: 包模式（Rust 包加载：入口 src/main.hc + 同目录兄弟文件）
hc run stage2 stage2\test\smoke.hc

:: 语义检查（stage1 checker 检查 stage2 全部源码）
hc run stage1\checker.hc stage2\src\main.hc

:: 链路（stage1 interp 执行 stage2 编译器；args[1] = 编译目标）
hc run stage1\interp.hc stage2\src\main.hc stage2\test\smoke.hc

:: token 对照（K1 格式，= hc lex）
hc lex stage2\test\smoke.hc
hc run stage1\interp.hc stage2\src\main.hc --dump-tokens stage2\test\smoke.hc
```

## 阶段状态

| 阶段 | 文件 | 状态 | 说明 |
|---|---|---|---|
| 入口/调度 | src/main.hc | ✅ S1 | 读目标参数 + 阶段调度；`io.fs.read_file` 宿主透传（S1 补入 stage1 interp） |
| 词法 | src/main.hc 词法节 | ✅ S2 | 提取完成；K1 对照 8/8 文件 MATCH（含 30KB 自身源码 6885 token，stage1 链路 464s ≈ 12 tok/s 嵌套解释固有速率） |
| 语法 | src/main.hc 语法节（S3） | 🔴 S3 | 从 stage1/interp.hc 内嵌 Parser 提取（含 switch 多模式臂、CharLit 十进制 prop 修复；rev_kw_map 改 if-chain——同 R2 规避） |
| 语义 | （S4 建） | 🔴 S4 | 从 checker.hc **stage2 副本**裁剪（stage1/checker.hc 冻结，K3 门禁 15 项不可破） |
| IR 模型 | （S5 建） | 🔴 S5 | IrModule/IrFunc/IrInst + kind 分发；对照 ir_inst.rs 49 变体圈定 ≤20 |
| lower | （S6 建） | 🔴 S6 | AST → IrInst |
| HBC2 编码 | （S7 建） | 🔴 S7 | 魔数 HBC2/v7；对照 encode.rs 子集；decode 丢 ret_ty/type_implements 无碍 |
| 闭环脚本 | test/bootstrap.bat | 🔴 S8 | interp 跑 stage2 → A.hbc；hc run A.hbc → B.hbc；fc /b 断言 A==B |
| 行为验证 | — | 🔴 S9 | A.hbc 执行输出 vs stage2 经 Rust 编译输出 |

## ✅ S2 缺陷复盘（已解决，2026-08-29）

**原登记的「类实例缺陷」不存在**——数据始终完好，是**打印/编码缺口**的假象：

1. `append_value` 无 vec 分支：`{}` 打印 Vec 值输出为空（Rust 参考 display 为 `[元素, ...]`）——已补
2. `Vec.as_slice` 未实现：返回 void（Rust 语义 = 收集 Int(0..=255) 元素为字节 → String）——已补
3. **CharLit props 编码缺陷**（真根因）：`get_prop` 的引号剥离启发式把 `"` 值字节当作引号剥掉、`|` 值被分隔符截断——`'"'`/`'|'` 字面量求值为 0，字符串/位或分派失效。**修复 = CharLit 值改存十进制文本**（`append_int`），绕开 `|key=value` 编码的特殊字节问题
4. 诊断教训：① `{}` 打印切片值需经 `Vec.as_slice()`（str 语义），直接打切片值会走数组格式化；② 循环变量勿用 `pi`（π 内置常量，同名声明被静默遮蔽——本次再犯）；③ stdout 重定向到文件为块缓冲，timeout 杀进程丢缓冲 → 「零输出」假象

**性能特征**：stage1 链路（嵌套解释）≈ 12 tok/s（30KB/6885 token = 464s）——非缺陷，为嵌套解释固有成本；自举链（S8）与 A.hbc 产物不受影响（编译产物原生执行）。

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
