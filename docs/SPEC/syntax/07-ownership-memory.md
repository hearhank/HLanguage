# 07 所有权与内存

> 大模块：所有权与内存 | 对齐状态：**✅ 对齐完成（2026-08-30，J1 裁决 + ADR-0035）** | 初稿：2026-08-30
>
> 事实基础：**ADR-0030**（所有权转移指针形态，2026-08-29，**现行权威**）、ADR-0025（显式所有权 + defer，2026-08-25）、ADR-0003（内存模型基础）、ADR-0021（分配器接口）、ADR-0022 §3（box 装箱 Q7/Q14）、历史 `06-06-ownership.md`（已废弃）、tag1 实现（`semantic/check.rs` owned_stack、`tag1/hc/tests/owned_check.rs` 13 测试）。
> 证据总库：`tag1/hc/tests/owned_check.rs`、`tag1/hc/tests/thread_capture.rs`、`tag1/hc-rt`（defer 栈/Boxed 释放）。

## 7.1 三轴权限模型（正交）

- 规则：变量三轴**正交可组合**——可读（默认）/ 可写（`mut`）/ 拥有（`owned`，类型前缀）：

| 形态 | 语义 |
|---|---|
| `T` | 只读（值/不可变引用） |
| `mut T` | 可写，无所有权 |
| `owned T` | 拥有所有权（必须 `defer` 或 `move`，§7.2） |
| `owned *T` / `owned *mut T` | 拥有 + 只读/可写指针（参数与返回的转移载体，§7.3） |

### 7.1.1 参数/字段标注模型（K1 裁决 + ADR-0036，2026-08-30）

- 语法：**`owned` 修饰符写在名称前**（参数与类型字段位置）：
  - 参数：`fn main(owned args: *mut Vec<String>) !void { }`
  - 字段：`class ABC { pub owned x: *mut T }` 或 `class ABC { pub owned x: mut T }`
  - var 声明保持**类型前缀**形态（`var x: owned T`，`01` §1.9.2）——两个位置两种语法，不混用。
- **所有权按形态判定**（修订 ADR-0025 参数表）：

| 形态 | 所有权 | 说明 |
|---|---|---|
| `T` | **必定拥有** | 裸值形态（值类型 = 副本归属；堆句柄 = 归属转移，受 AllocSource 门控） |
| `mut T` | **必定拥有** | 可写值形态（**修订 ADR-0025「mut T = 可写无所有权」**；`mut T` 作为类型形态为新文法 → backlog #16） |
| `*T` | 不一定 | 只读借用；`owned` 名称前缀 → `owned *T` 拥有 |
| `*mut T` | 不一定 | 可写借用；`owned` 名称前缀 → `owned *mut T` 拥有 |
| 值类型 + `owned` | **编译错误** | 栈/连续类型无所有权（ADR-0025 规则 4；诊断 → backlog #16） |

- `owned` 前缀与裸值形态叠写（如 `owned x: mut T`）= 允许（显式强调，语义等同裸形态）；`owned T` 值形态参数废除令（ADR-0030）由本模型取代。
- AllocSource 门控不变：Arena 分配 / `global` 变量不可作为拥有语义传参（ADR-0030 决策 4）。
- **实现落地（2026-08-31，backlog #16 完成）**：`owned` 名称前缀与 `mut T` 类型形态已在 parser/semantic 落地（`parse_params`/`parse_type`/`FieldDecl.owned`/`Param.owned`/`Type::MutValue`）；值类型 + owned 诊断（参数/字段两位置，`owned_eligible_st`）；main 签名 `owned args: *mut Vec<String>` 已接受。**参数位置 owned 的 defer 义务不强制**（owned 参数登记 move 资格但不进 defer 义务检查——main 注入参数无释放点，义务规则待裁决）。类型前缀 `owned T`（var 位置）语义不变。
- 状态：⚠️ 参数 defer 义务规则待裁决；其余 ✅
- 证据：裁决 K1 + ADR-0036（2026-08-30）；`hc/tests/frontend.rs` k1_* 组（2026-08-31）

## 7.2 `owned` 变量与 defer 义务（ADR-0025）

- 规则：
  - **作用域不自动释放任何资源**——堆对象/句柄/连接统一 `defer` 显式释放。
  - **`owned` 变量必须匹配 `defer` 或 `move`**，否则编译错误（Phase 2 error 级已落地，非 warning）。
  - `owned` 变量只能 move **一次**（affine）；move 后**冻结**（§7.3）。
  - **栈 / Arena 分配无所有权**：栈上标量/Continuous、Arena 分配对象不需要 `defer`，由栈/Arena 统一管理。
- 状态：✅ 已实现
- 证据：`semantic/check.rs` `check_block` L265-278（作用域退出检查「未匹配 defer/move 的 owned 变量」→ Diagnostic::error）；`owned_check.rs`

```hc
fn run(alloc: *Alloc) !void {
    var mut doc: owned *mut Document = alloc.init(Document);
    defer doc.*.close(alloc);          // 显式释放（或 move 转出）
    // 无 defer 且无 move → 编译错误
}
```

## 7.3 `move` 所有权转移（ADR-0030 指针形态）

- 规则：
  - 调用点形态：`move &t`（只读拥有转移）/ `move &mut t`（可写拥有转移）；**`move t` ≡ `move &mut t` 字面别名**（对非 `mut` 变量报错——不做自动降级改写）。
  - **冻结语义**（取代旧「原绑定仍可访问」）：move 后原变量**冻结**——后续读/写 = 编译错误；**重新赋值复活**（use-after-move 检查已实现）。
  - **分配来源判定**（AllocSource）：`alloc.init`（全局分配器）→ 可 move；**Arena 分配、`global` 变量 → 禁止 move**（内存由 Arena/全局对象管理）。
  - 闭包 `move |x| ...` 捕获形态保留（捕获语义，与所有权正交，`05` §5.6）。
- 状态：✅ 已实现（Rust semantic 2026-08-29：冻结 + 字面别名 + 三形态 move）
- 证据：ADR-0030 决策 2/3/4；`parser/expr.rs` `parse_unary` L218-229（Move 表达式）；`semantic/check.rs`（use-after-move 冻结）；`owned_check.rs`

```hc
fn take(doc: owned *mut Document) void { ... }
var mut d: owned *mut Document = alloc.init(Document);
take(move &mut d);        // 可写拥有转移
// d.*.x                  // ✋ 编译错误：d 已冻结（use-after-move）
d = alloc.init(Document); // 重新赋值 → 复活
```

## 7.4 返回与所有权

- 规则：
  - `owned *T` / `owned *mut T` 返回**合法**（所有权随返回转出，defer 义务转移给调用方）。
  - **返回形态自动推断推迟**（ADR-0030 决策 5：签名写 `T`、函数体 return 堆分配或 `&mut expr` 自动推断 `owned *mut T`——本轮只做检查面，自动推断推迟；与 H1「返回类型必须显式」衔接：基类型显式书写，`owned` 形态推断待实现）。
  - **引用逃逸检测**：`return &x`（局部引用）报错——保持现状。
- 状态：⚠️ 检查面 ✅，自动推断 ⏳（ADR-0030 明示推迟）
- 证据：ADR-0030 决策 5/6 + Consequences（「返回推断本轮只做检查面，自动推断推迟」）

## 7.5 分配器与 Arena（ADR-0021 / ADR-0003）

- 规则：
  - **全局分配器 `alloc`**：`alloc.init(T)` / `alloc.init(T{...})` 构造；带**泄漏追踪**（按分配行号报告）；**配对销毁 `alloc.destroy(x)` ✅（backlog #14① 已落地，2026-08-31）**：Bytes 分配注销同源泄漏记录（IR 侧按尺寸配对 / hc-rt 侧弱引用计数）；Class/Arr 置空占位（所有权标记，值随最后一个引用消亡）；LLVM 后端为 no-op 子集（无字节级 free 挂钩）。
  - **拥有变量的 defer 义务**：`var x: owned *mut T = alloc.init(...)` 后 `defer alloc.destroy(x);` 为标准释放闭环（`semantic/check.rs` owned_stack 义务检查对接）。
  - **Arena**：`Arena.init(alloc)` 构造；真实 bump 分配 + 块链表（G1）；`deinit` 批量归还 backing；Arena 分配的对象**无所有权**（归 Arena，禁止 move）；标准库方法集见 `04-stdlib` 体系（`08-mem-allocator-design.md` 设计文档为准绳）。
  - 箱/集合与分配器交互：装箱携带 alloc（三字宽第三字，§7.6）。
- 状态：⚠️ 部分——Arena bump/追踪 ✅；`alloc` 配对销毁 ✅（#14①，Box 显式销毁形式 = `alloc.destroy(x)`，backlog #15 改造待做）；Arena 详细方法集核对归标准库文档
- 证据：`ir/method.rs` `call_alloc_method_ir` "destroy"；`hc-rt/src/interp/call.rs` `(Value::Alloc, "destroy")`；`codegen/llvm/body.rs` `alloc.destroy` no-op；`hc/tests/frontend.rs` `b14_alloc_destroy_semantics_ok`

## 7.6 box / unbox 装箱（ADR-0035，2026-08-30，取代 ADR-0022 Q7/Q14）

- 规则：
  - `box(表达式)` = **内建方法**：采用 **alloc（全局分配器）分配内存**；返回 **`owned T`**（T = 实参类型）。
  - **接收变量必须带 `owned` 标注**——无标注 = 编译错误（如 `var x: owned i32 = box(input);`）。
  - 装箱变量遵守 §7.2 显式义务：**必须 `defer` 显式销毁或 `move` 转出**；**作用域自动释放（RAII）废除**（ADR-0035 决策 3）——IR 侧帧级 Boxed 自动释放集移除。
  - 显式销毁的调用形式已定（随 backlog #14① 落地，2026-08-31）：**`alloc.destroy(x)`**（与 alloc.init 对称；box 装箱的销毁形式同此，改造随 backlog #15）。
  - `unbox(v)` 取回值并消费装箱（所有权转移）。
  - 底层表示：三字宽胖指针（data + vtbl + alloc）为实现层细节保留；`*IShape` 接口胖指针同机制（`06` §6.2，box 值与接口收窄的交互随实现核对）。
- 状态：⚠️ 规范已定（实现改造 → backlog #15；LLVM 用户类型动态分派 ⚠️ 见 `06` §6.2）
- 证据：ADR-0035；`ir/runtime.rs` L526-527（旧自动释放集，待移除）；`call_unbox_builtin` L3615-3626

```hc
var input: i32 = 11;
var x: owned i32 = box(input);   // alloc 分配；接收变量必须 owned 标注
// defer <显式销毁 x>（形式随 backlog #14①）或 move x 转出
```

## 7.7 defer / errdefer 执行语义（ADR-0025）

- 规则：`defer` 作用域退出执行（**正常路径与错误路径均执行**）；多 defer **LIFO**（Q21）；`errdefer` 仅错误路径；defer 语句在书写位置入栈（词汇可见、可预测）。
- 状态：✅ 已实现
- 证据：ADR-0025 defer 语义节；`02` §2.17（语法）；`hc-rt` defer 栈实现（ADR-0025 影响范围 3）
- ❌ 历史形态作废：ADR-0003/0005 的「作用域退出自动递归销毁」已被 ADR-0025 明文取代。

## 7.8 绑定级只读与已知缺口

- 规则：
  - **绑定级默认只读（A 方案）= 1.x 待办**（ADR-0015 决策 2）：语义层 `VarInfo` 无 `mut_` 检查——`var t = ...; t[0,0] = 5` 今日可编译；密封表（`init_with`）不依赖此项（`04` §4.8）。
  - **字段可写性**：字段不支持 `mut` 标注（G3，`04` §4.1）；字段可写性模型随绑定级只读一并裁决。
  - `&T` 借用形态（`03` §3.3 F2 核查注）：`&x` 产出只读视图；与 owned 冻结的交互（对冻结变量 `&mut` 取地址应报错）待核对。
- 已知缺口（实现待对齐 → backlog #14 组，2026-08-31 已全部落地）：
  1. ~~`alloc.init` 无配对 `destroy`~~ ✅ `alloc.destroy(x)`（#14①，见 §7.5）
  2. ~~字段赋值不触发 move 检查~~ ✅ `doc.tag = move &mut tag;` 形态已支持：Assign 对 Move 实参解包指针载体比对，冻结登记照常（`semantic/check.rs` check_stmt Assign）
  3. ~~`join()`/`detach()` 不触发 move 检查~~ ✅ 句柄消耗模型：join/detach 后重复调用 = 编译错误；`is_done()` 等状态查询放行（`semantic/mod.rs` ThreadState.consumed + `infer.rs` check_call 拦截）
  4. ~~NonArena 自动跟踪已回退（按 AllocSource 判定替代）~~ ✅ 判定面固化测试（`hc/tests/frontend.rs` `b14_arena_and_global_move_rejected`：Arena/global/值类型 move 均报错）

## 7.9 变更记录（相对旧 06-06-ownership.md）

| 变更 | 依据 |
|---|---|
| 模型权威更新：ADR-0030（2026-08-29）取代 ADR-0025 值语义部分 | ADR-0030 Status: accepted |
| `move` 改指针形态：`move &t`/`move &mut t`；`move t` = 字面别名（非 mut 报错） | ADR-0030 决策 2 |
| **冻结语义**：use-after-move = 编译错误、重新赋值复活（取代「原绑定仍可访问」） | ADR-0030 决策 3 |
| `owned T` 值形态参数废除（`owned *T`/`owned *mut T` 为准） | ADR-0030 决策 1 |
| AllocSource 判定成文（global/Arena 禁 move） | ADR-0030 决策 4 |
| defer 义务 = error 级（Phase 2 已落地，非 warning） | `check.rs` + owned_check.rs |
| box 模型重定：`box(expr)` → `owned T`、alloc 分配、显式销毁；RAII 废除 | **裁决 J1** + **ADR-0035**（2026-08-30，取代 ADR-0022 Q7/Q14）→ backlog #15 |
| 已知缺口组移入 backlog #14 | `06-06` 已知限制 5 条（实现核对后保留 4 条） |

## 7.10 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| J1 | box 装箱模型 | **`box(expr)` = 内建方法，alloc 分配，返回 `owned T`；接收变量必须 owned 标注；显式销毁（defer/move），RAII 废除**——ADR-0035 取代 ADR-0022 Q7/Q14 | §7.6、ADR-0035、backlog #14①/#15、`06` §6.2 |
