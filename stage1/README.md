# stage1 — H 自举第一阶段（E7）

本目录包含 H 语言自举（E7）的第一阶段实现：用 H 语言编写 H 编译器。

## 进度

| 模块 | 文件 | 状态 |
|------|------|------|
| K1 Lexer | `lexer.hc` | ✅ 已完成（6621 token 零 diff） |
| K2 Parser | `parser.hc` | ✅ 已完成（性能已优化，解析自身 ~1s） |
| **K3 语义分析** | **`checker.hc`** | **🟡 推进中（8/11 任务完成）** |
| K4 后端 | — | 🔴 待实现 |
| K5 自举闭环 | — | 🔴 待实现 |
| K6 可复现构建 | — | 🔴 待实现 |

## K3 语义分析（`checker.hc`）

`stage1/checker.hc` 是 H 版语义分析器（语义阶段），与 Rust 参考实现（`hc check`）逐项对照，差异即 bug。

### 已完成任务（8/11）

| # | 任务 | 状态 |
|---|------|------|
| 1 | Checker 骨架 + lexer/parser 内嵌 | ✅ |
| 2 | 核心类型系统（SType/IntWidth/VarInfo/FnSig/AllocSource） | ✅ |
| 3 | 符号表 + 作用域栈（扁平 Vec + scope_sizes 模式） | ✅ |
| 4 | 收集阶段（class/enum/union/interface/fn 登记） | ✅ |
| 5 | 未定义名称检测（check_expr/check_stmt/check_ident） | ✅ |
| 6 | 类型解析 ty_of + 变量声明类型检查 | ✅ |
| 7 | 表达式类型检查（is_compatible + 二元运算符） | ✅ |
| 8 | 语句类型检查（if/while/for 条件检查） | ✅ |
| 9 | 所有权分析（分配来源/move/引用逃逸） | ✅ |
| 10 | 错误集分析（错误集推断/错误码表） | ✅ |
| 11 | 集成验证（全部语料对照 + 修复不匹配） | ⏳ |

### 对照测试

测试位于 `tag1/hc-tools/tests/k3_checker.rs`，当前 6 项测试全部通过：

| 测试 | 语料文件 | 说明 |
|------|---------|------|
| `fn_basic_matches_rust_reference` | `10-fn-basic.hc` | 函数声明 |
| `var_decl_matches_rust_reference` | `11-var-decl.hc` | 变量声明 |
| `simple_expr_matches_rust_reference` | `13-expr.hc` | 表达式 |
| `type_decl_matches_rust_reference` | `15-types.hc` | 类型声明 |
| `undefined_name_detected` | `17-undefined-simple.hc` | 未定义名称检测 |
| `if_while_matches_rust_reference` | `12-if-while.hc` | if/while 语句 |
| `ownership_move_detected` | `21-ownership.hc` | 所有权分析（move 检测） |
| `error_set_detected` | `22-error-set.hc` | 错误集分析（错误字面量检测） |

运行方式：`cargo test --release -p hc-tools --test k3_checker`

### 用法

```bash
# 运行语义分析器
hc run stage1/checker.hc <file.hc>

# 输出格式：
#   成功：OK
#   失败：error:line:col: message
```

### 语料文件

`stage1/corpus/` 目录包含 19 个测试语料文件（01–20），覆盖词法、语法、语义各阶段。