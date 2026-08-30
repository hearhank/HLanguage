# 13 `@` 内建函数全集

> 大模块：`@` 内建 | 对齐状态：**✅ 对齐完成（2026-08-30，无待裁决项；一处诊断缺口移交 backlog #20）** | 初稿：2026-08-30
>
> 事实基础：定案 Q-S2/Q-S3/Q-S6/Q12、历史 `06-04-functions.md`（@ 内建段，已废弃）、tag1 实现（`ir/builtin.rs` `call_builtin` 总分派 L4496-4544、`is_type_arg_pos`、`lexer/mod.rs` @ 词法）。
> 关联：断言五件套 `12` §12.2；box/unbox `07` §7.6；spawn `11` §11.2；溢出策略 `02` §2.2。
> 范围注：`box`/`unbox`/`copy`/`spawn`/`expect*` 等为**普通调用形态内建**（无 `@` 前缀）——各自归属模块收口，本模块附清单（§13.7）。

## 13.1 语法与解析

- 规则：
  - `@名称(实参)`——`@` 后跟标识符（词法 token `AtBuiltin(name)`，不与用户标识符冲突）；**必须带调用形态**（无裸引用，`02` §2.18）。
  - 编译器维护内建名注册；类型参数位（如 `@sizeOf(i32)`、`@atomicLoad(T, ...)` 首参）为编译期类型名，不对运行时求值（`is_type_arg_pos`）。
  - ⚠️ **未知名内建当前静默返回 Void**（`call_builtin` 兜底 `_ => Void`）——诊断缺口：应报「未知内建」→ backlog #20。
- 状态：✅ 已实现（未知名诊断 → backlog #20）
- 证据：`lexer/mod.rs` L43-47；`ir/builtin.rs` L4542

## 13.2 内省

| 内建 | 签名 | 说明 |
|---|---|---|
| `@sizeOf(T)` | `usize` | 类型字节大小（struct 与 C ABI 一致，ADR-0022） |
| `@alignOf(T)` | `usize` | 自然对齐 |
| `@offsetOf(T, 字段)` | `usize` | 字段偏移（两参：类型 + 字段名） |
| `@typeOf(expr)` | `String` | 表达式类型名的字符串形式（如 `"i128"`） |
| `@intFromEnum(e)` | 整数 | 枚举 → 底层整数值 |
| `@enumFromInt(i)` | 枚举 | 整数 → 枚举变体 |

- 状态：✅ 已实现
- 证据：`ir/builtin.rs` `call_introspection_builtin`（L4516-4518 分派；ExitType 内建枚举特判 L3872）

## 13.3 类型转换

| 内建 | 签名 | 说明 |
|---|---|---|
| `@intCast(T, v)` | T | 整数跨宽度/符号转换（显式，`03` §3.2.1） |
| `@ptrCast(T, p)` | T | 指针类型转换 |
| `@alignCast(T, p)` | T | 指针对齐提升 |
| `@ptrFromInt(i)` / `@intFromPtr(p)` | 双向 | 指针 ↔ 整数地址 |

- ⏳ **浮点转换未实现**：`@floatCast`（f16/f32/f64 互转）、`@intToFloat`/`@floatToInt`——随 F1 浮点宽度化（backlog #5）一并落。
- ⏳ `@bitCast` / `@mulAdd`：1.x 按需（历史 06-04 口径维持）。
- 状态：✅ 已实现（5 项）；浮点族 ⏳（backlog #5 关联）
- 证据：`call_cast_builtin`（L4520-4522 分派）；`is_type_arg_pos` L1750-1754

## 13.4 诊断

| 内建 | 签名 | 说明 |
|---|---|---|
| `@panic(消息)` | never | 不可恢复终止（abort，无 unwind、不执行 defer——`08` §8.7） |
| `@compileError(消息)` | never | **编译期错误**——求值到该调用即中止编译（常量求值/类型函数守卫用） |

- 状态：✅ 已实现
- 证据：`call_diag_builtin` L3959-3970

## 13.5 原子与 volatile（Q-S3 定案）

| 内建 | 签名 | 说明 |
|---|---|---|
| `@atomicLoad(T, p, order)` | T | 原子载入 |
| `@atomicStore(T, p, v, order)` | void | 原子存储 |
| `@atomicRmw(T, p, op, v, order)` | T（旧值） | 原子读改写（op：add/sub/…/exchange） |
| `@volatileLoad(p)` / `@volatileStore(p, v)` | — | 易失访问（不被优化器消除） |

- 内存序：`relaxed` / `acquire` / `release` / `acq_rel` / `seq_cst`（默认 `seq_cst`）；并发语义见 `11` §11.8。
- 状态：✅ 已实现
- 证据：`call_atomic_builtin` / `call_volatile_builtin`（L4523-4524 分派）

## 13.6 溢出算术（Q-S6 定案）

| 内建 | 签名 | 说明 |
|---|---|---|
| `@addWithOverflow(a, b)` | `(T, bool)` | 加法溢出原语（value, overflow） |
| `@subWithOverflow(a, b)` | `(T, bool)` | 减法 |
| `@mulWithOverflow(a, b)` | `(T, bool)` | 乘法 |

- 状态：✅ 已实现
- 证据：`call_math_builtin`（L4526-4528 分派）；元组返回（`12` 测试 `try expect_eq(@intCast(i32, 7), 7)` 邻证）

## 13.7 非 `@` 内建清单（交叉引用）

| 名称 | 归属 |
|---|---|
| `box` / `unbox` | `07` §7.6（ADR-0035 owned 模型） |
| `copy` | `07`（深复制语义；所有权衔接待该模块核对） |
| `spawn` | `11` §11.2 |
| `Pipe` / `Tee` / `Funnel` / `Hub`（容器构造） | `11` §11.6（弃用过渡期） |
| `sqrt` / `min` / `max` | 数学工具（标准库面，归 `04-stdlib-scope.md` 校订） |
| `fmt_int` / `fmt_float` / `read_u64_le` | 格式化工具（同上） |
| `sort` / `binary_search` | 排序工具（同上） |
| `parse_int` / `parse_float` | 解析工具（同上） |
| `skip_space` / `peek` / `advance` / `is_digit` / `parse_number` | 解析器辅助内建（71-recursive-parser 语料特化 ⚠️——是否语言级内建或语料辅助，随标准库边界校订） |
| `expect` / `expect_eq` / `expect_neq` / `expect_error` / `expect_eq_slices` | `12` §12.2 |
| `String.fromInt` / `@intToStr` | 字符串方法/别名（`04` §4.7 方法集） |

- ❌ `@cImport`：E3 外部库接入，⏳ 未实现（历史 06-04 口径维持）。

## 13.8 变更记录（相对旧 06-04 §@ 内建段）

| 变更 | 依据 |
|---|---|
| 全集清单化：6 内省 + 5 转换 + 2 诊断 + 2 volatile + 3 原子 + 3 溢出算术（共 21 个 `@` 内建） | `call_builtin` 总分派 L4503-4544 |
| `@typeOf` 返回 String（类型名字符串化）补录 | `call_introspection_builtin` + LLVM 测试 |
| `@volatileLoad/Store` 补录（旧文档未列） | L4523 |
| `@intToStr` 别名补录 | `call_dotted_implicit` L3418 |
| 浮点转换族（@floatCast 等）标 ⏳（随 F1/backlog #5）；@bitCast/@mulAdd 维持 1.x | 分派缺失 + 历史 06-04 |
| 未知名 `@` 内建静默 Void → 诊断缺口 backlog #20 | `call_builtin` L4542 兜底 |
| `@cImport` ❌→⏳ 维持（E3） | 历史 06-04 |
| 非 `@` 内建清单化并交叉引用（禁止双写） | `call_builtin` 其余分支 |

## 13.9 待裁决清单

无——清单型模块，全部条目按实现证据直接对齐。
