# IR 降级必须响亮失败：子集外构造一律 error.Unsupported

`hc build` 与 `hc run --ir` 只覆盖 M3.1–Phase 6 子集。对子集外的构造（for/switch/break/continue/defer/errdefer、闭包/集合/指针/字段/索引/解构/取地址/函数引用/块表达式、实例方法调用、区间糖、全局/常量声明等曾被静默处理的项），`ir::lower` 一律返回 `error.Unsupported`，**带行列 + 「请用默认 tree-walking 模式」提示**，进程非零退出。

背景：早期存在 P0 缺陷——子集外构造被静默降级为 `void` 占位 / 丢语句，产生「看似成功实则错」的产物。本决策规定降级**不静默丢弃**，`hc build` / `hc run --ir` 直接报错；tree-walking 默认路径零改动。未实现的原生内建/方法在运行时以 `error.NotBuiltin` / `error.NoMethod` 响亮中止（原生 ABI 留后续全标准库）。

这条策略是「原生后端是子集」这一已知取舍的安全阀：宁可报错，不可误编译。
