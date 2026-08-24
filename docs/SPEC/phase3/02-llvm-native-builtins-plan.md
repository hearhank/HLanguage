# LLVM 原生内建实现计划

## 背景

当前 LLVM 后端使用统一带标签值 `%Value = { i32 tag, i128 data }`，所有运算通过运行时 helper 函数做动态 tag 分派。原生内建的目标是用原生 LLVM IR 指令替代这些 helper 调用。

## 划分原则

- **Phase 3（容易）**：纯 codegen 改动，不改 `%Value` 表示，不改函数 ABI，不改 `IrInst`/`IrFunc` 结构。每个任务改一处方法/加一个模板，通过检查 LLVM IR 字符串可测。
- **Phase 4（困难）**：需要改 `%Value` 表示、函数 ABI、类型跟踪、单态化等基础架构。

---

## Phase 3 任务（第三阶段完成）

以下是每个任务 ≤1h，可独立测试验证。

### P3-1: 条件分支内建原生（JumpIf/JumpIfNot/JumpIfNull/JumpIfErr）

**文件**: `body.rs`  
**当前**: 
```llvm
%v = load %Value, %Value* %sp.{temp}
%c = call i1 @hc_truthy(%Value %v)  ; 或 @hc_is_null, @hc_is_err
br i1 %c, label %L{label}, label %{fb}
```

**改为**: 对 `JumpIfNull` 直接 `extractvalue %Value %v, 0` + `icmp eq i32 %tag, 1`；对 `JumpIfErr` 直接 `icmp eq i32 %tag, 6`；对 `JumpIf`/`JumpIfNot` 保持 `hc_truthy`（因为 truthy 涉及多种类型判断）。

实际上 `JumpIfNull` 和 `JumpIfErr` 是纯 tag 检查，可直接内联。
`JumpIf`/`JumpIfNot` 仍需要 `hc_truthy`（复杂逻辑：null→false, 0→false, 空串→false 等），留到 P4。

**测试**: 验证 `extern fn` 的 null/err 分支不产生 `@hc_is_null`/`@hc_is_err` 调用。

**预估**: 30min

### P3-2: math.* 数值函数内联

**文件**: `body.rs` (`call_math` 方法)  
**当前**: `math.nan`/`math.inf`/`math.inf_neg` 已内联（直接发位模式常量）。`math.sqrt`/`math.abs`/`math.floor`/`math.ceil`/`math.round`/`math.pow` 走 helper。

**改为**: 
- `math.sqrt` → 直接 `call double @llvm.sqrt.f64(double %v)`（已有 declare）
- `math.abs` → `call double @llvm.fabs.f64(double %v)`（已有 declare）
- `math.floor` → `call double @llvm.floor.f64(double %v)`（已有 declare）
- `math.ceil` → `call double @llvm.ceil.f64(double %v)`（已有 declare）
- `math.round` → `call double @llvm.round.f64(double %v)`（已有 declare）
- `math.pow` → `call double @pow(double, double)`（libm）

各 helper 模板（`hc_sqrt`, `hc_abs` 等）可删除。

**测试**: 验证 `math.sqrt(x)` 等不产生 `@hc_sqrt` 调用，而是 `@llvm.sqrt.f64`。

**预估**: 30min

### P3-3: @sizeOf/@alignOf 标量类型内联

**文件**: `body.rs` (`call_builtin` 方法)  
**当前**: 已内联（`scalar_size_native`, `align_native` 直接返回常量），但未知类型走 `abort_feature`。

**改为**: 扩展 `scalar_size_native` 和 `align_native` 覆盖更多类型（`i8`/`u8`/`i16`/`u16`/`f16`/`i32`/`u32`/`f32`/`i64`/`u64`/`isize`/`usize`/`f64`/`i128`/`u128`/`f128`/`bool`/`String`/`Vec`/`Map`/`Deque`/`Table`/`Allocator`），当前已覆盖。

**不需要额外改动**，但可增加测试覆盖。

**测试**: 验证 `@sizeOf(i32)` 产生 `i128 4`，`@sizeOf(f64)` 产生 `i128 8`。

**预估**: 15min

### P3-4: @intCast 标量类型内联

**文件**: `body.rs` (`call_builtin` 方法)  
**当前**: 调用 `hc_intcast(%Value %v, i128 {min}, i128 {max})`，helper 做动态 tag dispatch + 范围检查。

**改为**: 对已知源类型和目标类型，直接发射 LLVM  trunc/sext/zext 指令。但当前 `@intCast` 在 IR 层级已丢失源类型信息——只能从运行时 `%Value` 的 tag 推断。所以需要先加**类型槽表**（P4-1）。

**移到 Phase 4**。

### P3-5: @ptrFromInt/@intFromPtr 内联

**文件**: `body.rs` (`call_builtin` 方法)  
**当前**: 已内联（`extractvalue` + `build_store`），无需改动。

**测试**: 验证 `@ptrFromInt(n)` 产生 `T_PTR` tag。

**预估**: 15min

### P3-6: 断言 helper 内联

**文件**: `body.rs` (`call_builtin` 方法) + `helpers.rs`  
**当前**: `hc_expect`, `hc_expect_eq`, `hc_expect_neq`, `hc_expect_error`, `hc_expect_eq_slices` — 5 个 helper 模板。

**改为**: 这些断言逻辑复杂（涉及 `hc_eq`/`hc_truthy`/`hc_is_err` 调用），不能简单内联。但可以**合并**减少模板体积。保持现状。

**移到 Phase 4**。

### P3-7: K4 @volatileLoad/@volatileStore 内联

**文件**: `body.rs` (`call_builtin` 方法) + `helpers.rs`  
**当前**: 调用 `hc_volatile_load`/`hc_volatile_store` helper。

**改为**: 直接发射 LLVM `load volatile`/`store volatile`：

```llvm
; 当前
%res = call %Value @hc_volatile_load(%Value %v)

; 改为：直接 inline
%data = extractvalue %Value %v, 1
%ptr = inttoptr i128 %data to %Value*
%val = load volatile %Value, %Value* %ptr
store %Value %val, %Value* %sp.{temp}
```

**删除** `hc_volatile_load` 和 `hc_volatile_store` 模板。

**测试**: 验证 `@volatileLoad(p)` 产生 `load volatile`。

**预估**: 30min

### P3-8: 标量 binop 内联（Add/Sub/Mul/Div/Mod 整数路径）

**文件**: `body.rs` (`bin` 方法)  
**当前**: 所有 `IrBinOp::Add` 等走 `hc_add`/`hc_sub`/`hc_mul`/`hc_div`/`hc_mod` helper。

**改为**: 保留 `%Value` 装箱，但跳过 tag dispatch。对已知整数类型（从 IR 类型推断），直接发射 LLVM 整数指令：

```llvm
; 当前
%res = call %Value @hc_add(%Value %va, %Value %vb)

; 改为（int 路径）
%ta = extractvalue %Value %va, 0
%tb = extractvalue %Value %vb, 0
%da = extractvalue %Value %va, 1
%db = extractvalue %Value %vb, 1
%is_int = icmp eq i32 %ta, 2
%is_both_int = and i1 %is_int, %is_int   ; 实际上需要检查两个都是 int
; 简化：直接假设 int（从 IR 类型推断）
%r = add i128 %da, %db
%v0 = insertvalue %Value { i32 0, i128 0 }, i32 2, 0
%v1 = insertvalue %Value %v0, i128 %r, 1
store %Value %v1, %Value* %sp.{temp}
```

**注意**：此优化有风险——如果类型推断错误，会产生静默错误。需要配合**类型槽表**（P4-1）安全实施。

**移到 Phase 4**。

### P3-9: 比较操作内联（Eq/Ne Lt/Le Gt/Ge 整数路径）

**文件**: `body.rs` (`bin` 方法)  
**当前**: 比较操作走 `hc_eq`/`hc_lt` helper。

**同 P3-8**，需要类型槽表安全实施。

**移到 Phase 4**。

### P3-10: 一元操作内联（Neg/Not/BitNot）

**文件**: `body.rs` (`un` 方法)  
**当前**: 调用 `hc_neg`/`hc_not`/`hc_bitnot` helper。

**同 P3-8**，需要类型槽表。

**移到 Phase 4**。

---

## Phase 4 任务（第四阶段完成）

### P4-1: 类型槽表（Type Slot Table）

**文件**: `body.rs` 新增 `type_of_slot: HashMap<usize, Type>` 或类似结构  
**难度**: 高

**描述**: 在 `emit_func` 中，遍历 IR 指令推断每个槽的类型。需要：
- 从 `IrFunc.param_ty` 推断参数槽类型
- 从 `IrInst::Const` 推断常量槽类型
- 从 `IrInst::Bin` 的操作数和结果类型传播
- 处理条件分支后的类型合并（phi 节点）

**测试**: 验证 `fn add(a: i32, b: i32) i32 { return a + b; }` 中槽 0/1/2 类型为 `i32`。

**预估**: 2-3h

### P4-2: 标量运算原生 ABI（无装箱）

**文件**: `body.rs`, `emit.rs`, `preamble.rs`  
**难度**: 高

**描述**: 当函数所有参数和返回值都是标量类型时，跳过 `%Value` 装箱：
- 函数签名为 `i32 @hc_fn0(i32 %p0, i32 %p1)` 而非 `%Value @hc_fn0(%Value %p0, ...)`
- 内部指令直接操作 `i32`/`i64`/`f64` 等原生类型
- 调用点根据被调函数类型选择原生或装箱调用路径
- 需要 thunk 层：`%Value @hc_fn0_boxed(%Value %p0, %Value %p1) { %r = call i32 @hc_fn0_native(...); call %Value @hc_box_int(%r) }`

**测试**: 验证 `fn add(a: i32, b: i32) i32 { return a + b; }` 产生 `add i32` 指令。

**预估**: 5-7h

### P4-3: 聚合类型映射 LLVM struct

**文件**: `preamble.rs`, `body.rs`, `helpers.rs`  
**难度**: 高

**描述**: 将 H 的 `class`/`enum`/`array` 映射到 LLVM struct 类型：
- `class Point { x: i32, y: i32 }` → `%Point = type { i32, i32 }`
- `hc_field`/`hc_store_field` → `getelementptr` + `load`/`store`
- `hc_index`/`hc_store_index` → `getelementptr` + `load`/`store`
- `hc_make_class` → 直接分配 + 初始化 struct
- 删除 `%ClassObj`, `%ArrObj`, `%EnumObj`, `%Field` 等运行时类型

**需要 P4-1 完成**（需要知道 class 的字段类型）。

**测试**: 验证 `var p = Point { x: 1, y: 2 }; return p.x;` 产生 `getelementptr`。

**预估**: 5-7h

### P4-4: 错误值原生处理

**文件**: `body.rs`, `emit.rs`  
**难度**: 高

**描述**: 错误值不再通过 `T_ERR` tag 嵌入 `%Value`，而是用 LLVM 错误返回机制（如 `{ i32, i1 }` 或双返回值）：
- `try` 调用展开为条件分支
- `catch` 块接收错误值
- 错误传播路径避免运行时 tag 检查

**测试**: 验证 `fn foo() !i32 { return error.Bad; }` 产生条件分支而非 `hc_is_err`。

**预估**: 5h

### P4-5: 迭代器原生

**文件**: `body.rs`, `helpers.rs`  
**难度**: 中

**描述**: 将 `for` 循环展开为 LLVM 原生循环：
- `hc_iter_make` → 直接构造迭代器结构
- `hc_iter_next` → 直接比较+递增
- `hc_iter_write_back` → 直接存储

**需要 P4-3 完成**（迭代器涉及聚合类型）。

**测试**: 验证 `for x in 0..10 { ... }` 产生 LLVM 原生循环（`icmp`+`br`+`add`）。

**预估**: 3-4h

### P4-6: 打印/io 原生

**文件**: `body.rs`, `helpers.rs`, `preamble.rs`  
**难度**: 中

**描述**: 将 `io.print(...)` 展开为直接 `printf`/`write` 调用，而非经过 `hc_write_*` helper 链：
- `io.print("hello")` → `call i32 @puts(i8* @".str.0")`
- `io.print("x = {}", 42)` → `call i32 @printf(i8* @".fmt", i64 42)`
- 不需要 `hc_write_bytes`, `hc_write_strz`, `hc_write_u128_base` 等 8 个 helper

**测试**: 验证 `io.print("hello")` 产生 `@puts` 调用而非 `@hc_write_value`。

**预估**: 3-4h

### P4-7: 数学库内建原生

**文件**: `body.rs`, `helpers.rs`  
**难度**: 低

**描述**: `min`/`max`/`sqrt`/`box`/`copy`/`fmt_int`/`fmt_float`/`read_u64_le` 等内建函数：
- `min`/`max` → `icmp slt` + `select`（整数）或 `fcmp olt` + `select`（浮点）
- `sqrt` → `call double @llvm.sqrt.f64`（已完成 P3-2）
- `box` → 堆分配 + 拷贝（保持现状）
- `copy` → `deep_copy` 或恒等（保持现状）
- `fmt_int`/`fmt_float` → `sprintf` 直接调用

**需要 P4-1 完成**（需要类型信息）。

**测试**: 验证 `min(a, b)` 产生 `icmp` + `select`。

**预估**: 3h

### P4-8: 删除运行时 helper 模板

**文件**: `helpers.rs`, `preamble.rs`  
**难度**: 低

**描述**: 逐一删除已被内联替换的 helper 模板。每个 helper 删除可独立验证（重新编译 + 跑测试）。

**测试**: 所有 LLVM 测试通过。

**预估**: 1h（批量删除 + 验证）

---

## 汇总

| 任务 | 阶段 | 难度 | 预估时间 | 前置依赖 |
|------|------|------|---------|---------|
| P3-1: 条件分支内建（Null/Err） | Phase 3 | 低 | 30min | 无 |
| P3-2: math.* 内联 | Phase 3 | 低 | 30min | 无 |
| P3-3: @sizeOf/@alignOf 扩展 | Phase 3 | 低 | 15min | 无 |
| P3-5: @ptrFromInt/@intFromPtr 测试 | Phase 3 | 低 | 15min | 无 |
| P3-7: @volatileLoad/@volatileStore 内联 | Phase 3 | 低 | 30min | 无 |
| P4-1: 类型槽表 | Phase 4 | 高 | 3h | 无 |
|   P4-1a: 定义+参数/常量填充 | 已完成 | ✅ | 45min | 无 |
|   P4-1b: Load/Bin/Un/Store/AddrSlot/Deref 传播 | 已完成 | ✅ | 45min | 无 |
|   P4-1c: CallBuiltin/MakeClass/MakeEnum/MakeArr + 比较→bool | 已完成 | ✅ | 45min | 无 |
| P4-2: 标量运算原生 ABI | Phase 4 | 高 | 7h | P4-1 |
| P4-3: 聚合类型 LLVM struct | Phase 4 | 高 | 7h | P4-1 |
| P4-4: 错误值原生处理 | Phase 4 | 高 | 5h | P4-1 |
| P4-5: 迭代器原生 | Phase 4 | 中 | 4h | P4-3 |
| P4-6: 打印/io 原生 | Phase 4 | 中 | 4h | P4-2 |
| P4-7: 数学库内建原生 | Phase 4 | 中 | 3h | P4-1 |
| P4-8: 删除运行时 helper | Phase 4 | 低 | 1h | P4-2~P4-7 |

**Phase 3 小计**: 4 个任务，约 2h
**Phase 4 小计**: 8 个任务，约 34h
