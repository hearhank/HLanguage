# C7 原生 ABI 函数值/闭包设计定案（Phase 8 原生改造）

> 2026-08-22 定案 · 2026-08-23 实施（C7 = 组 G4b 定案 A 的 Phase 8 原生改造，grill-with-docs 访谈，3 子项全确认）。关联：SPEC [06-04-functions.md](../SPEC/06-04-functions.md)（闭包规范）、[06-10-concurrency.md](../SPEC/06-10-concurrency.md)（spawn 原生边界）、[01-unimplemented-features.md](../phase3/01-unimplemented-features.md)（C7 条目）、[02-syntax-rules.md](../phase3/02-syntax-rules.md)（§2.2 函数/闭包）。

## 背景

- interp/IR 侧：`MakeClosure` / `FnRef` / `CallIndirect` **已实现**（示例 10-functions / 21-closures / 48-iterator-chain 在 interpret/IR 全绿）
- 原生 LLVM 侧（`tag1/hc/src/llvm.rs` L5674-5681）：这三类指令 `abort_feature("notcallable")` → **编译模式响亮拒绝**（G4b 定案 A，Phase 4 临时取舍——不静默误编译）
- 原生后端用 tagged `%Value`（`{ i32 tag, i128 载荷 }`）表示一切值；闭包需新表示
- spawn callee 以 FnRef 传递 → 原生线程程序也在 `NotCallable` 拒绝（spawn 原生子集边界，G4b 定案 A）
- 设计已定方向（G4b 定案 A 保留「响亮拒绝」直到真实支持）；本 ADR 定 Phase 8 原生 ABI 的表示与调用约定
- **2026-08-23 实施**：C7-1/2/3 全落地 LLVM 原生后端（`FnRef`/`MakeClosure`/`CallIndirect` 替换 `abort_feature`；闭包函数发射 + `hc_eq_plain` 新增 tag 处理）。 spawn 内建仍经 `error.NotBuiltin` 拒绝（待 Phase 7 内建改造）

## 决策

### C7-1 闭包/函数引用表示 → 复用 tagged `%Value`，新增闭包 tag ✅

1. **函数引用 `FnRef`** = `ptrtoint` 函数指针存 `%Value` 载荷（`T_FN` tag=14）；间接调用 = `inttoptr` + `call`
2. **闭包 `MakeClosure`** = **胖闭包对象**（`{ i8* fn_ptr, i8* env_ptr }`，堆上分配），`%Value` 载荷存其指针——**`T_CLOSURE` tag=15**
3. **捕获环境 env** = 捕获变量的 `%Value` 数组（堆分配），闭包函数开头从 env 加载到对应槽

### C7-2 调用约定 → 复用 `%Value` 参数/返回值通道 ✅

1. **`CallIndirect`**：按 tag 分派——`T_FN`：`inttoptr` + 间接调用；`T_CLOSURE`：解包胖闭包 + env 隐首参 + 显式参数
2. 参数/返回值一律 `%Value`（与既有 H 调用约定一致，零新机制）；闭包函数 = `(%Value %env, %Value %arg0, ...)`
3. **零动态分发**：函数值 = 直接函数指针（无虚表）；静态已知时 LLVM 可内联

### C7-3 与 spawn / 现有边界联动 ✅

1. spawn callee 以 FnRef 传 → `FnRef` 路径已通（`ptrtoint`），`spawn` 内建仍经 `error.NotBuiltin` 拒绝
2. `NotCallable` compile mismatch 归零；`FnRef`/`MakeClosure`/`CallIndirect` 不再 `abort_feature`
3. `hc_eq_plain` 新增 T_FN/T_CLOSURE payload 身份比较；`hc_typeof` 分支已预留（n_closure 标签）

## 理由

- C7-1 复用 tagged `%Value`：与既有原生值模型同构（零新表示通道），闭包 tag 区分「带环境」与「裸函数指针」；env 结构复用 interp 捕获槽语义，interp/IR/LLVM 三后端捕获语义一致
- C7-2 复用 `%Value` 调用通道：与 H 既有调用约定一致，零新 ABI；隐藏 env 首参是 Fat closure 的通行做法（Zig/C++ lambda 同源）；零动态分发保持「无隐藏控制」
- C7-3 边界联动：真实支持替换「响亮拒绝」，符合 G4b 定案 A 的初衷（拒绝是为了不静默误编译，而非永久禁能）；三后端一致性承诺（双模式核心）要求原生最终覆盖 interp 已有能力
