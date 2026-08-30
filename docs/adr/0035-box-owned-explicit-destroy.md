# box 装箱改为 owned 显式销毁模型

2026-08-30 裁决（语法规范对齐会话，J1）。本 ADR 取代 ADR-0022 Q7/Q14 中「box 自动释放（RAII）」「装箱返回指针类型」的条款。

## 决策

1. `box(表达式)` 是内建方法：采用 **alloc（全局分配器）分配内存**；返回 **`owned T`**（T = 实参类型）。
2. **接收变量必须带 `owned` 标注**（如 `var x: owned i32 = box(input);`）——无标注 = 编译错误。
3. 装箱变量遵守 ADR-0025 显式义务：**必须 `defer` 显式销毁或 `move` 转出**；**作用域自动释放（RAII）废除**——IR 侧 Boxed 集的退出自动释放同步移除。
4. 显式销毁的具体调用形式随 alloc 配对 destroy（backlog #14①）确定。

## 影响

- 语义/IR：box 返回类型改 `owned T`；移除帧级 Boxed 自动释放集；defer/move 义务覆盖装箱变量。
- 接口胖指针（三字宽 data+vtbl+alloc）作为实现层表示保留；box 值与接口指针的收窄交互随实现核对。
