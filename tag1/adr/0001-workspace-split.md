# tag1 垂直切片按三 crate 拆分：hc / hc-rt / hc-tools

tag1（第一部分「最小功能集」）的 Rust 工作区拆为三个 crate，单向分层，无环：

```
hc  (编译器前端：lexer / parser / AST / 诊断 / 语义 / IR / 字节码 / LLVM 发射)
 └─ hc-rt  (运行时：Value 值模型 + tree-walking 解释器 + 内建/最小标准库，依赖 hc)
     └─ hc-tools  (工具链 CLI：hc run / hc test / hc build / hc check / hc errors，依赖 hc 与 hc-rt)
```

选择三 crate 而非单 crate 或更多 crate：前端是纯编译、零运行时依赖的库，运行时是解释执行语义的载体，工具链只是把两者组装成 CLI 的薄层——三者生命周期与测试粒度不同，拆分让 `cargo test -p <crate>` 能独立验证，也让「编译器」与「运行时」的边界在类型层面强制。未再细分（如把 lexer/parser/语义各拆一 crate），因为第一阶段目标是垂直切片而非架构展示，过度切分徒增心智负担。

约束：工作区零外部依赖（Rust 标准库 + `zig cc` 作为可选进程级外部工具），保证自举路径不被第三方牵制。
