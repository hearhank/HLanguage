# H 语言规范：错误处理

> 对应实现模块：07 第一块语言系统 M2 语义（错误集）/ M4 运行时（错误码、@panic）。

## error union

```hc
fn parse(data: &[u8]) ParseError!Value { ... }   // 显式错误集 E!T
fn f(x: &[u8]) !i32 { ... }                      // 推断错误集 !T
var v = try parse(data);                          // try 传播
var x = expr catch 默认值;                        // catch 处理
var x = expr catch |err| { ... };
if (e!) |v| else |err| { ... }                    // 双向捕获（Q9：必须成对，错误显式处理）
```

- `E!T` / `!T` 类型表达「可能出错」：成功时为 T、失败时为错误值；与 optional（`?T`「可能没值」）**正交**
- **错误值引用（Q13 定案）**：`error.Name` 在任何上下文可引用（**错误名全局唯一**）；可 return/比较/switch 分支匹配（`error.NotFound => ...`）；返回显式错误集的函数中 `return error.X` 未在集合内 → 编译报错；`anyerror` 上下文任意错误名可用
- **`!T` = 推断错误集（Q-S8 定案）**：错误集由编译器从函数体收集（`return error.X` + `try` 传播的实际返回集）——与显式错误集语义一致、调用方可穷举；递归/泛型无法收集时退化 `anyerror`（编译器提示显式标注）；`anyerror` 仍仅接口契约（Q34）
- 错误集联合：`A || B`（26-error-set-union 示例）
- **忽略错误仅 `catch |_| {}`**（Q11 定案：不提供 `catch {}` 简写，忽略必须显式）

## 错误值运行时表示（2026-08-14 定案）

- 错误 = **全局唯一整数错误码**（编译器维护「错误名 ↔ 码」表，跨包统一；Q13 错误名全局唯一）；**编码 = 「包 ID + 包内码」（L5 定案：高位 = 编译单元包 ID，低位 = 包内错误序——静态链接与动态库/插件场景均无冲突，M6 实现细化）**
- error union 运行时表示 = 错误码 + 成功标记（**Zig 式——成功路径零额外负载**）
- Debug 附带错误源位置（返回点）；`anyerror` = 任意码（64 位空间）

## 不可恢复终止

- **`@panic("消息", 位置)`（Q-S2 定案）**：不可恢复运行时错误（Debug 悬垂标记开启时访问已标记悬垂指针等）——打印消息 + 位置（Debug 带堆栈），**abort 终止**；**不执行 defer 清理**、**无 unwind/recover**（回卷是隐式控制流，不引入——与「没有隐藏控制」一致）
- 测试环境：测试函数内 panic → 该测试记 FAIL（不终止整个 `hc test`）；Release 同样 abort（消息可精简）；panic 钩子留 1.x
- 与 error union（可恢复）正交：可恢复错误走 `E!T`，不用 panic

## 退出映射

- **`ExitType` = 语言内建枚举（L3 定案，2026-08-14）**：`enum ExitType { Exit, Error }`（编译器提供，落 M4 运行时；`Exit`/`Error` 两变体固定，扩展走 `io.exit` 的 code 参数）+ `io.exit(t: ExitType, code: u8) !void`——`Exit` 正常静默 / `Error` 错误退出（打印错误标记/位置）
- `main` 返回 error → 等效 `io.exit(ExitType.Error, 1)`；正常返回 → `io.exit(ExitType.Exit, 0)`；测试失败 → 非零退出码（Q-T2）

## 测试断言（归 std.debug，测试函数内隐式可用）

- `expect(cond)` / `expect_eq(a, b)`（失败输出期望 vs 实际）/ `expect_neq(a, b)` / `expect_error(error.e, expr)`（R-3）/ `expect_eq_slices(a, b)`（失败输出长度 + 首个差异位置）——全部返回 `anyerror!void`（`try` 传播即失败）
- `expect_eq` 支持 `==` 可比较类型（含 String 内容比较，Q3）
