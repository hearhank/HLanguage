# 10 元编程

> 大模块：元编程 | 对齐状态：**✅ 对齐完成（2026-08-30，无待裁决项；C5-① 移交 backlog #17）** | 初稿：2026-08-30
>
> 事实基础：ADR-0012（comptime 类型值）、ADR-0013（script 块语义，已随移除失效）、ADR-0018 C5（已知边界定案）、`12-script-redesign.md`（2026-08-23）、历史 `06-09-meta.md`（已废弃）、tag1 实现（`ir/comptime.rs`、`parser/decl.rs` comptime 块、`tag1/hc/tests/comptime.rs`）。
> 原则：**无宏**（12.22 定案——宏不进入语言，与「没有隐藏控制」一致）。

## 10.1 comptime 类型函数

- 规则：
  - `fn 名称(T: type, ...) type { return 类型表达式; }`——返回 `type` 的函数 = **类型函数**（编译期执行，monomorphization + 惰性缓存）。
  - 体支持的类型表达式形态（实现边界）：`return [n]T;`（数组类型值，长度可为字面量或 `comptime_int` 参数引用）、`return T;`（类型参数/固定名直通）；其它表达式形态 → 诊断「类型函数：体不支持该形态」。
  - **嵌套/递归实例化**（D3）：`PairPair<i32>` / `LinkedList<T>` 自引用——深度遍历 + `instantiating` 登记期守卫（防无限递归）；IR 声明式无初值补默认值。
  - **已知边界 C5-①**（ADR-0018，2026-08-22 定案）：内建泛型外层嵌套具体化 `Vec<List<i32>>`——预期行为已定案（= `Vec<List<@i32>>`），**当前仍退化裸名 `Vec`，修复待实施** → backlog #17。
- 状态：✅ 已实现（C5-① ⏳ → backlog #17）
- 证据：`ir/comptime.rs` `is_type_fn`（L12-23）/`instantiate`（L214+ 实参分类与诊断）/`map_type_apps`（嵌套守卫）/`eval_type_expr`（L323-333 体形态限制）；`tag1/hc/tests/comptime.rs`

```hc
fn List(T: type) type { ... }                          // 类型函数
fn ArrayLen(T: type, n: comptime_int) type {
    return [n]T;                                       // [3]i32 等
}
var buf: ArrayLen(i32, 3) = [0, 0, 0];
```

## 10.2 comptime 值参数（`T: type` / `n: comptime_int`）

- 规则：
  - 类型函数参数分两类：`T: type`（类型参数，须收类型实参）与 `n: comptime_int`（编译期整数值参数，须收整数字面量）——实参按参数序对齐，错配 = 编译诊断（含实参个数）。
  - `comptime_int` 实参不落 IR（防御性编码/解码对称，字节码 tag 9）。
- 状态：✅ 已实现
- 证据：`ir/comptime.rs` `instantiate` 参数分类（L218-265）；`codegen/bytecode/encode.rs` L164-169

## 10.3 comptime 值函数（D4c）

- 规则：**参数含 `T: type`、但返回非 `type`** 的普通函数 = comptime 值函数——调用点**编译期求值折叠**（如 `array_len(i32)` = 4）；自递归守卫防死循环；类型值仅编译期存在（D5：IR/原生无类型值与调用残留——`collect_value_fns` + `try_fold_comptime_value_call`）。
- 状态：✅ 已实现
- 证据：`ir/comptime.rs` `is_comptime_value_fn`（L40-55，与 `is_type_fn` 正交判定）；D5 一致性测试（`d5_comptime_value_fn_consistent`）

## 10.4 comptime 块

- 规则：
  - `comptime { ... }` **声明级**块——装载期**受限 Interp** 求值；**结果丢弃**（不替换源码）；求值失败 = **编译错误**（带块内位置 + 所属块位置——与运行时错误同一机制，2026-08-14 定案：作用域/函数/块均为可返回错误的执行单元）。
  - `types` 元数据对象（`types.fields/type/all`）可见性按块位置（H5 定案）——comptime 块可见全部类型；**实现可用性 ⚠️ 待核对**（受限 Interp 的 types 面尚未证实）。
- 状态：✅ 已实现（D2 最小切片；types 元数据 ⚠️）
- 证据：`parser/decl.rs` L271-281（comptime 块注释「装载期受限 Interp 求值、结果丢弃、失败 = 编译错误」）；测试 5 项端到端（历史组 D D2）

## 10.5 anytype（D4b 完整语义）

- 规则：`anytype` 参数（类型 = `Type::Infer`）——调用点按**实参具体类型实例化**；返回 `anytype` 解析为函数体 return 表达式在具体绑定下的类型；具体化键形如 `max_value<@i32,i32>`；类型误配 = 诊断（如 `max_value(2.5, 1.5)` 期望 f64 报 i32 误配）。anytype 函数返回**运行时值**（非类型函数——与 `is_type_fn` 正交）。
- 状态：✅ 已实现
- 证据：`ir/comptime.rs` L27-28（正交注释）；`semantic` `match_overloads`（类型层具体化）；测试 +6（历史组 D D4b）

## 10.6 comptime_int / comptime_float 惰性与折叠

- 规则：类型名识别（`03` §3.2.5）+ **常量折叠**：comptime 块/值函数内算术在编译期求值；收窄溢出/类型不匹配在**收窄点**诊断；`expect_eq` 断言可折叠比较。
- 状态：✅ 已实现
- 证据：组 D D4（`comptime_int` 折叠 +7 测试、`comptime_float` 惰性宽度 +5 测试，历史记录）

## 10.7 script / .hs（❌ / ⏸）

- 规则：
  - ❌ `script { }` 块已从 `.hc` 移除（2026-08-23，`12-script-redesign.md`）——声明位置出现即报错并指引迁移（`01` §1.2.1）。
  - **`.hs` 脚本文件** = 脚本能力的新载体：脚本生成管样板（数据定义驱动序列化/校验/存储，Q37/Q38 定案；`types.fields` 驱动）；**实现 ⏸ 自举后**（`00-index.md` 排除列表——脚本相关实现推迟）。
  - 降级闸门（Q-S10，历史）：脚本生成若成本超预期 → 降级为编译期执行 H 子集函数（comptime 内联）——该闸门随 `.hs` 排期保留。
  - 脚本输入机制（Q23）：脚本产物 = 生成的代码字符串就地替换；脚本用 H 字符串操作拼接生成代码，**无第二语言**。
- 状态：.hc script ❌（诊断 ✅）；.hs ⏸ 自举后
- 证据：`parser/decl.rs` L264-270（script 硬错误 + 迁移指引）；`12-script-redesign.md`

## 10.8 变更记录（相对旧 06-09-meta.md）

| 变更 | 依据 |
|---|---|
| 「script 块 = E1 已实现」表述修正：❌ 已移除（旧文档头部未同步 2026-08-23 决策） | `12-script-redesign.md` + `parse_decl` 硬错误 |
| 组 D 全系列实现证据收口（D1–D5：类型函数/值参数/comptime 块/anytype/值函数/三后端一致性） | `ir/comptime.rs` + 历史 06-09 定案记录 |
| C5-① 嵌套具体化退化明确 ⏳ → backlog #17；C5-② 无限大小类型 = 编译错误成文 | ADR-0018 |
| `types` 元数据对象可用性标注 ⚠️（受限 Interp 面待核对） | H5 定案 vs 实现边界 |
| 序列化定制通道（Q37/Q38）随 `.hs` ⏸ 排期 | 排除列表 |
| `context` 关键字不在词法表（ADR-0026 表述修正已在 `09` 收口） | `01` §1.2.1 |

## 10.9 待裁决清单

无——本模块全部条目按 ADR-0012/0018 与实现证据直接对齐。
