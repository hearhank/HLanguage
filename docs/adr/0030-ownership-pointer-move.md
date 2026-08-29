# 所有权转移改为指针形态（owned *T / move &t），废弃值语义 move

**Status**: accepted（2026-08-29，grilling 会话定案）
**关联**: `CONTEXT.md` §5（2026-08-29 修订）、`docs/SPEC/phase1/06-06-ownership.md`、`docs/SPEC/phase4/06-k5-execution-plan.md`

## 决策

所有权与读、写正交不变（可读 / 可写 `mut` / 拥有 `owned` 三轴），但**所有权转移的载体从值语义改为指针形态**：

1. **参数侧**：转移所有权的参数声明为 `owned *T`（只读拥有）或 `owned *mut T`（可写拥有）；`owned T` 值形态参数废除。
2. **调用点**：`move &t`（只读拥有转移）/ `move &mut t`（可写拥有转移）；**`move t` 是 `move &mut t` 的字面别名**（对非 `mut` 变量报错——糖有糖的边界，不改写降级）。`move |x|` 闭包捕获形态保留（捕获语义，与所有权正交）。
3. **冻结语义（取代旧「原绑定仍可访问」）**：move 后原变量冻结——后续读/写编译错误，重新赋值复活（use-after-move 检查）。
4. **分配来源判定**：全局分配器（`alloc.init`，`alloc` 是全局分配器而非 arena）→ 可 move；**Arena 分配、`global` 变量 → 禁止 move**（虽为堆分配，内存由 Arena/全局对象管理）。检查器按 `AllocSource` 判定（checker.hc 的 `enum AllocSource` 基建已在）。
5. **返回推断**：签名写 `T`，若函数体 `return` 堆分配构造（`alloc.init(...)`）或 `return &mut expr`，自动推断为 `owned *mut T`（用户仍只写 T）；裸值 return（标量/切片）不推断，保持值语义。调用方拿到 `owned *mut T`，方法调用走指针自动解引用（K4 语料 07 事实标准）。
6. **`owned` 变量声明保留**（`var owned t = ...`，defer 义务锚点不变），只废除「`move t` 值转移」路径；引用逃逸检测保持现状（`return &x` 局部引用仍报错），`owned *T`/`owned *mut T` 返回合法（所有权随返回转出）。

## Considered Options

- **值语义 move（旧模型，2026-08-25）**：`owned T` 参数 + `move t`——与指针自由模型（`*mut` 可复制、多写者合法）存在概念张力，且「变量本身不变、原绑定仍可访问」使 use-after-move 不可检查 → 废弃。
- **move 后不冻结（仅销毁责任转移）**：调用方与接收方共同持有 → 两人共同销毁责任，恰是 ownership 要防的 → 弃。
- **move t 对非 mut 变量自动降级 move &t**：糖会变形，诊断困难 → 弃，采字面等价。
- **owned 变量声明废除、推断化**：defer 义务失去显式锚点，违背「销毁责任在创建处可见」→ 弃。

## Consequences

- **实现面**：Rust semantic（owned 检查改写 + use-after-move + 分配来源判定 + 返回推断；hc-tools owned_check 4 测试改写）——作为 parity oracle 在 K6 前完成（K5 期间择机）；stage1 checker.hc 同步**推迟到 K6**（stage2 编码纪律规避所有权构造，不阻塞 K5）。
- **狗粮零破坏**：stage1 四件套对 move/owned 零使用（仅关键字表），语料 01–10 不受影响；K3 语料 21-ownership.hc（`var y = move x` 值语义）需按新规则重写。
- **对 K5**：无阻塞；`06-k5-execution-plan.md` 风险登记已更新。K6 一致性以本 ADR 为基准。
- spec 同步：`CONTEXT.md` §5 与 `06-06-ownership.md` 已按本 ADR 修订。
