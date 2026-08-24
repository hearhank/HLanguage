# Table 密封表（init_with 构造）+ H 绑定级只读未实现

> 2026-08-22 定案（Table 设计会话 grill-with-docs，Q1–Q4 全推荐）。关联：SPEC [06-03-extended-types.md](../SPEC/06-03-extended-types.md)（Table 段）、[01-unimplemented-features.md](../phase3/01-unimplemented-features.md)（C1 / C4 条目）、[02-syntax-rules.md](../phase3/02-syntax-rules.md)（§4.3 多索引 / §7.1 序列化）、查询手册 [H Language.md](../H%20Language.md) §9.3。

## 背景

- **Table<T> 设计**（2026-08-22 会话）：行视图 `t[i]`、单元格读写/复合赋值 `t[i,j]`、扁平迭代、`len()/cols()`、to_bytes 双前缀、空表合法、copy 深复制、嵌套 `Table<Table<T>>`、指针元素替换规则
- **密封构造需求**：`Table<*mut T>`（可写指针元素）若构造后仍可写，破坏读安全——需**构造期写入 + 构造后只读**的密封语义
- **关键发现**：H 文档宣称**绑定级默认只读**（Rust 式），但语义层**未实现**——`VarInfo` 无 `mut_` 字段，`var t = ...; t[0,0]=5` 今日可编译

## 决策

1. **B 方案：密封表 `init_with`**——`Table<T>.init_with(alloc, rows, cols, cb)` 回调 `\|i, j, cell: *mut T\|` 内写格，构造完成返回**编译期强制只读表**：直接赋值 / 复合赋值 / `&mut t` 一律编译错误，**不可解除密封**（无 `unseal`）。`Table<*T>` 只读指针元素可普通 `init`（天然不可替换元素）；普通表当前允许替换，待 C4 绑定级只读门控
2. **绑定级只读（A 方案，全局绑定只读）记 1.x 待办**——第三阶段不做：需语义层 `VarInfo` 增 `mut_` 字段 + 赋值检查 + `AddrOf` 校验，且示例/测试套件几十处 `var x = 1; x = 100` 需迁移；密封表只读由 `init_with` 自身语义保证，不依赖全局绑定只读
3. **Q3 暂缓**：CONTEXT.md Table 条目不拆分（当前单条目够用）

## 理由

- 选密封（init_with）而非「构造后手动检查」：编译器强制，杜绝遗漏；回调内 `*mut T` 恰好提供构造期写能力，与「写时安全」分离
- 绑定级只读推迟：A 方案为跨语义层 + 全测试套件的大迁移，收益独立于 Table 本身；密封表已覆盖当下最需只读的场景（可写指针元素表）
- 实施影响（C1）：`semantic.rs check_index` 放宽 1/2 索引（1 索引 → 行视图 Slice）、`interp.rs eval_assign` 修多索引写 bug（当前单索引静默退化整行赋值）、`ir.rs` 链式降级（字节码/LLVM 零改动）、新测试（多索引写 / 行视图 / init_with 密封 / 复合赋值 / to_bytes 往返 / 空表）
