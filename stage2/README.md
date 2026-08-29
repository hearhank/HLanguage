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

:: 编译（S6/S7：多文件合并 lower + HBC2 编码；声明按文件序合并）
hc run stage2 --emit-hbc out.hbc stage2\src\main.hc stage2\src\ir.hc stage2\src\lower.hc stage2\src\encode.hc stage2\src\lexer.hc stage2\src\parser.hc stage2\src\checker.hc

:: 链路（stage1 interp 执行 stage2 编译器；args[1..] = 编译参数）
hc run stage1\interp.hc stage2\src\main.hc --emit-hbc out.hbc %SRC%

:: 产物执行（HBC2 VM；编译器自身可再编译任意 .hc）
hc run out.hbc --dump-ast target.hc

:: token 对照（K1 格式，= hc lex）
hc lex stage2\test\smoke.hc
```

## 阶段状态

| 阶段 | 文件 | 状态 | 说明 |
|---|---|---|---|
| 入口/调度 | src/main.hc | ✅ S1 | 读目标参数 + 阶段调度；`io.fs.read_file` 宿主透传（S1 补入 stage1 interp） |
| 词法 | src/lexer.hc | ✅ S2 | 提取完成；K1 对照 8/8 文件 MATCH（含 30KB 自身源码 6885 token，stage1 链路 464s ≈ 12 tok/s 嵌套解释固有速率）；同命名空间扁平共享已落地（ADR-0031 loader 修复） |
| 语法 | src/parser.hc | ✅ S3 | 提取完成；两级 AST dump 对照 9/9 MATCH（含 stage2 全部自身源码，parser.hc AST 8037 行）；AstDumper 同步提取（--dump-ast 模式） |
| 语义 | src/checker.hc | ✅ S4 | 从 checker.hc **stage2 副本**裁剪（stage1/checker.hc 冻结，K3 门禁 15 项不可破） |
| IR 模型 | src/ir.hc | ✅ S5 | IrModule/IrFunc/IrInst/IrConst class + kind 分发；指令集 25 变体（对照 ir_inst.rs 49）；无 Map（平行 Vec + 线性查） |
| lower | src/lower.hc | ✅ S6 | AST → IrInst；对齐 tag1 lower_impl 子集语义；子集外构造响亮诊断；探针 probe_ir.hc 验证 |
| HBC2 编码 | src/encode.hc | ✅ S7 | 魔数 HBC2/v7；字段序=decode.rs 读回序；排序确定性；**V1 达成：A.hbc == B.hbc 逐字节（304166B）** |
| 闭环脚本 | test/bootstrap.bat | 🟡 S8 | 脚本已入库；interp 全链待跑（数小时量级，登记基线后闭环） |
| 行为验证 | — | 🔴 S9 | 待 S8 后（S9-mini 已过：编译版 --dump-ast vs Rust parse 仅 ret: 渲染差异） |

## ✅ S2 缺陷复盘（已解决，2026-08-29）

**原登记的「类实例缺陷」不存在**——数据始终完好，是**打印/编码缺口**的假象：

1. `append_value` 无 vec 分支：`{}` 打印 Vec 值输出为空（Rust 参考 display 为 `[元素, ...]`）——已补
2. `Vec.as_slice` 未实现：返回 void（Rust 语义 = 收集 Int(0..=255) 元素为字节 → String）——已补
3. **CharLit props 编码缺陷**（真根因）：`get_prop` 的引号剥离启发式把 `"` 值字节当作引号剥掉、`|` 值被分隔符截断——`'"'`/`'|'` 字面量求值为 0，字符串/位或分派失效。**修复 = CharLit 值改存十进制文本**（`append_int`），绕开 `|key=value` 编码的特殊字节问题
4. 诊断教训：① `{}` 打印切片值需经 `Vec.as_slice()`（str 语义），直接打切片值会走数组格式化；② 循环变量勿用 `pi`（π 内置常量，同名声明被静默遮蔽——本次再犯）；③ stdout 重定向到文件为块缓冲，timeout 杀进程丢缓冲 → 「零输出」假象

**性能特征**：stage1 链路（嵌套解释）≈ 12 tok/s（30KB/6885 token = 464s）——非缺陷，为嵌套解释固有成本；自举链（S8）与 A.hbc 产物不受影响（编译产物原生执行）。微优化已落地（kw_of 首字母分桶 + lex_ident 内联 + env 长度预检）：同输入 76→66s（~13%）。字节码/原生执行路线因 IR/LLVM 对重类实例模式保真缺口暂不可用（详见 ADR-0031 后续）。

**多文件拆分已回归（ADR-0031 落地）**：src/ 同目录文件扁平互见（Rust loader 同命名空间扁平登记已修 + stage1 interp 包模式）；拆分实测：Rust 包模式 / stage1 链路 / checker 三链路全部贯通。

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

## lower/encode 新增陷阱（S6/S7 实测，2026-08-30）

- **方法必须在类体内**：`self: *mut Self` 自由函数无 Self 语义（`self.x + 1` 报操作数类型错）。
- **指针参需显式 `.*`**：`out: *mut Vec<u8>` 上 `out.append` 被拒，须 `out.*.append`（checker.hc append_bytes 同款）；`Vec` 赋值是引用语义（`var v = idxs` 报错），拷贝用 `copy(&idxs)`。
- **AstNode/Vec 引用类型不可重绑**：`var cur = e; cur = cur.children[0]` 被拒——递归下降替代。
- **`Vec` 无 `pop`**：回滚用 `remove(len - 1)`（checker.hc 同款）。
- **循环计数/if-let 缺省赋值一律 `var mut`**（Rust hc 强制）。
- **AtBuiltin token 文本不含 `@`**：parser 折叠为 `Ident("intCast")`——lower 须还原 `@intCast`（运行时内建名带 @），checker 两个名字都要认。
- **stage2 parser 类方法 = `Fn` + `method=类名` prop**（非 Rust dump 的 `Method` kind）；类字段 = `FieldDecl`。
- **若需 `v[i] = x`**：lower 已支持 StoreIndex（第 25 号指令），自举自身代码需要它。

## 一次性自举链（S8）

```bat
set SRC=stage2\src\main.hc stage2\src\ir.hc stage2\src\lower.hc stage2\src\encode.hc stage2\src\lexer.hc stage2\src\parser.hc stage2\src\checker.hc
hc run stage1\interp.hc stage2\src\main.hc --emit-hbc stage2\test\A.hbc %SRC%   REM interp 全链（数小时）
hc run stage2\test\A.hbc --emit-hbc stage2\test\B.hbc %SRC%                     REM A.hbc 自编译
fc /b stage2\test\A.hbc stage2\test\B.hbc                                        REM V1：字节级相等
```

跨宿主确定性已验证（S7）：宿主编译（tree-walking 执行编译器）产物 == A.hbc 自编译产物，逐字节相等（304166 B，21s）。
