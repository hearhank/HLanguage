# 08 错误处理

> 大模块：错误处理 | 对齐状态：**✅ 对齐完成（2026-08-30，无待裁决项）** | 初稿：2026-08-30
>
> 事实基础：定案 Q9/Q11/Q13/Q-S2/Q-S8/L3/L5、历史 `06-07-errors.md`（已废弃）、tag1 实现（`ir/lower_impl.rs` defer/ErrPath、`ir/builtin.rs` 诊断内建、`parser/mod.rs` 错误联合文法、`tag1/hc/tests/inferred_errors.rs`）。
> 关联文法：错误联合类型 `03` §3.6；错误集定义 `01` §1.10；try/catch/orelse 语法形态 `02` §2.10；switch 错误模式 `02` §2.15；errdefer `07` §7.7。本模块收口语义与运行时行为。

## 8.1 错误集定义

- 规则（文法定义见 `01` §1.10，此处收口语义）：
  - `const E = error{成员, ...};` 定义命名错误集；`const E = A || B;` 定义错误集联合。
  - **错误名全局唯一**（Q13）：`error.Name` 在任何上下文可引用——return/比较/switch 匹配/`catch` 绑定。
  - 返回显式错误集 `E!T` 的函数中 `return error.X` 且 X ∉ E → **编译报错**；`anyerror` 上下文任意错误名可用。
- 状态：✅ 已实现
- 证据：`parser/decl.rs` `parse_const`（错误集特例 L416-473）；历史 26-error-set-union 示例

## 8.2 错误联合类型（`E!T` / `!T`）

- 规则：
  - `E!T` = 显式错误集联合：成功时为 T、失败时为错误值；`!T` = **推断错误集**（Q-S8）：错误集由编译器从函数体收集（`return error.X` + `try` 传播的实际返回集），调用方可穷举；递归/泛型无法收集时退化 `anyerror`（编译器提示显式标注）。
  - `anyerror` = 任意错误码（仅接口契约用途，Q34）。
  - 与 optional（`?T`「可能没值」）**正交**——可恢复错误走 `E!T`，不用 optional/panic。
- 状态：✅ 已实现
- 证据：`parser/mod.rs` L114-126（ErrorUnion）；`tag1/hc/tests/inferred_errors.rs`（推断错误集专项）

## 8.3 错误值运行时表示

- 规则（2026-08-14 + L5 定案）：
  - 错误 = **全局唯一整数错误码**（编译器维护「错误名 ↔ 码」表，跨包统一）；编码 = **包 ID + 包内码**（高位包 ID、低位包内序——静态/动态链接均无冲突）。
  - error union 运行时 = 错误码 + 成功标记（**Zig 式：成功路径零额外负载**）；Debug 附带错误源位置（返回点）。
- 状态：✅ 定案（编码矩阵证据归 M6 运行时核对）
- 证据：历史 `06-07` 定案记录；`ir` 错误信号流（`JumpIfErr`/`ErrPath`）

## 8.4 错误传播与处理

- 规则（语法形态见 `02` §2.10）：
  - `try e`——错误值从当前函数返回（传播）；错误路径触发 errdefer（§8.6）。
  - `e catch |err| 体`——绑定错误对象处理；`e catch 默认值`——兜底。
  - **忽略错误仅 `catch |_| {}`**（Q11：不提供 `catch {}` 简写——忽略必须显式）。
  - 控制流兜底：`e catch return [expr]` / `e catch break` / `e catch continue`。
  - if 双向捕获：`if (e!) |v| ... else |err| ...`（Q9：必须成对，错误显式处理）；while 错误捕获 ⏳（E3，`02` §2.13）。
- 状态：✅ 已实现
- 证据：`ir/lower_impl.rs` `lower_expr` Try 分支 L1258-1268；`parser/expr.rs` catch L372-420；`parse_capture`（`_` 作标识符放行）

```hc
fn parse(data: &[u8]) ParseError!Value { ... }
var v: Value = try parse(data);                       // 传播
var x: Value = parse(data) catch Value.default;       // 兜底
parse(data) catch |err| { io.print(err.to_string()); };  // 绑定处理
parse(data) catch |_| {};                             // 显式忽略（唯一形态）
```

## 8.5 switch 错误匹配

- 规则：`error.Name` 作为 switch 模式（`02` §2.15）；多错误模式臂 `error.A, error.B => ...`；穷举规则同 switch（Q31）。
- 状态：✅ 已实现
- 证据：`parser/stmt.rs` `parse_switch_pattern` L477-482

## 8.6 defer 与错误路径（ErrPath 策略）

- 规则（IR Phase 6，对齐 oracle）：
  - 退出点三策略：**Never**（正常路径/`break`/`continue`——errdefer 不运行）/**Always**（`try` 错误返回——全部 defers 含 errdefer）/**Value(t)**（`return e` 运行期判定——错误值走 Always、否则 Never）。
  - defer 体不得含控制流指令（降级期硬错误——跳转会因重复发射冲突）。
- 状态：✅ 已实现
- 证据：`ir/lower_impl.rs` ErrPath 注释 L905-908、`emit_defers` L960-991、`lower_expr` Try 分支注释

## 8.7 `@panic` 不可恢复终止（Q-S2 定案）

- 规则：
  - `@panic(消息)`——不可恢复运行时错误：打印消息 + 位置（Debug 带堆栈），**abort 终止**；**不执行 defer 清理**、**无 unwind/recover**（回卷 = 隐式控制流，不引入）。
  - 可恢复错误走 `E!T`，与 panic 正交；panic 钩子留 1.x。
  - 测试环境：测试函数内 panic → 该测试记 FAIL（不终止整个 `hc test`）；Release 同样 abort（消息可精简）。
  - 实现注记：旧定案形如 `@panic("消息", 位置)`——实现当前仅消费消息参数，位置由抛出点自动附加（⚠️ 双参形态待核对/收敛）。
- 状态：✅ 已实现（双参形态 ⚠️ 核对注）
- 证据：`ir/builtin.rs` `call_diag_builtin` L3959-3970；`codegen/llvm/body.rs` `call_builtin` L912-921（@panic → 运行时 abort）

## 8.8 退出映射（ExitType，L3 定案）

- 规则：
  - **`ExitType` = 语言内建枚举**：`enum ExitType { Exit, Error }`——编译器提供，两变体固定（扩展走 `io.exit` 的 code 参数）。
  - `io.exit(t: ExitType, code: u8) !void`——`Exit` 正常静默 / `Error` 错误退出（打印错误标记/位置）。
  - `main` 返回 error → 等效 `io.exit(ExitType.Error, 1)`；正常返回 → `io.exit(ExitType.Exit, 0)`；测试失败 → 非零退出码（Q-T2）。main 形态本身归 `09-modules-entry.md`。
- 状态：✅ 已实现
- 证据：`ir/builtin.rs` L3872-3882（ExitType 内建枚举 L3）、`call_io_method_ir` L2482-2492（io.exit F2）、`lower_impl.rs` L1846-1854（ExitType 特判）；`ir/runtime.rs` main 返回映射注释

## 8.9 变更记录（相对旧 06-07-errors.md）

| 变更 | 依据 |
|---|---|
| 测试断言 API 移交 `12-testing.md`（本模块去重） | 禁止双写 |
| try 错误路径触发 errdefer 的 Always 策略成文 | `lower_impl.rs` ErrPath |
| `return e` 运行期分派（Value(t)）成文 | `emit_defers` L976-991 |
| @panic 双参形态标注核对注（实现仅消费消息） | `call_diag_builtin` |
| `catch |_|` 显式忽略的词法依据补录（`_` = 标识符） | `parse_capture` + `01` §1.1.2 |
| 错误集联合 `A \|\| B` 限定 const 特例（E1 裁决口径） | `02` §2.20 |
