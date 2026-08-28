# stage1 — H 自举第一阶段（E7）

本目录包含 H 语言自举（E7）的第一阶段实现：用 H 语言编写 H 编译器。

## 进度

| 模块 | 文件 | 状态 |
|------|------|------|
| K1 Lexer | `lexer.hc` | ✅ 已完成（6621 token 零 diff） |
| K2 Parser | `parser.hc` | ✅ 已完成（性能已优化，解析自身 ~1s） |
| **K3 语义分析** | **`checker.hc`** | **✅ 已完成（11/11 任务完成，13 项对照测试全部通过）** |
| K3.5 自检收敛 | — | 🟡 新增：checker 自检 stage1 三源不崩溃 ✅（2026-08-29），误报收敛待做（类/方法/字段建模不全） |
| K4 后端 | — | 🔴 待实现 |
| K5 自举闭环 | — | 🔴 待实现 |
| K6 可复现构建 | — | 🔴 待实现 |

## K3 语义分析（`checker.hc`）

`stage1/checker.hc` 是 H 版语义分析器（语义阶段），与 Rust 参考实现（`hc check`）逐项对照，差异即 bug。

### 已完成任务（11/11）

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
| 11 | 集成验证（全部语料对照 + 修复不匹配） | ✅ |

### 对照测试

测试位于 `tag1/hc-tools/tests/k3_checker.rs`，当前 13 项测试全部通过：

| 测试 | 语料文件 | 说明 |
|------|---------|------|
| `fn_basic_matches_rust_reference` | `10-fn-basic.hc` | 函数声明 |
| `var_decl_matches_rust_reference` | `11-var-decl.hc` | 变量声明 |
| `simple_expr_matches_rust_reference` | `13-expr.hc` | 表达式 |
| `type_decl_matches_rust_reference` | `15-types.hc` | 类型声明 |
| `undefined_name_detected` | `17-undefined-simple.hc` | 未定义名称检测 |
| `if_while_matches_rust_reference` | `12-if-while.hc` | if/while 语句 |
| `ownership_move_detected` | `21-ownership.hc` | 所有权分析（move 检测） |
| `reference_escape_detected` | `23-ref-escape.hc` | 引用逃逸检测（return &局部） |
| `error_set_detected` | `22-error-set.hc` | 错误集分析（错误字面量检测） |
| `type_error_detected` | `18-type-error.hc` | 类型错误检测 |
| `integration_strings_matches_rust_reference` | `04-strings.hc` | 字符串字面量对照 |
| `integration_undefined_matches_rust_reference` | `16-undefined.hc` | 带类型注解的未定义变量对照 |
| `integration_debug_files_matches_rust_reference` | `19-get-prop-test.hc`, `20-debug-ty.hc` | 调试文件对照 |

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

### 自检回归（2026-08-29）

**修复 `error.NoField at 0:0` 自检崩溃**：checker.hc 检查 lexer.hc / parser.hc / 自身时，解释器在 `type_of_expr` Call 分支响亮中止。根因（吃狗粮暴露）：

1. **漏解包可选值**（3 处）：`ty_of`（`self.types.get(name)`）、`type_of_expr` Ident 分支、Call 分支（`sig.ret_type`）把 `Map.get` 返回的 `?SType`/`?FnSig` 直接当非可选取字段。规范要求 `?T` 使用前显式解包（`06-02-types.md`）——补 `if (t) \|tt\| { return tt; }` 解包。解释器 NoField 属正确行为；Rust 语义检查器未在编译期抓住此错（`.field` 作用于 `?T` 不报错），登记为语义检查缺口。
2. **解析器丢弃 `\|payload\|` 绑定名**：`parse_if_stmt`/`parse_while_stmt` 解析载荷后仅存局部变量（lint 曾报「未使用变量 cap」）。现经 `node_add_prop` 存入 `payload`/`payload_err` 属性，`check_if`/`check_while` 在作用域内注册载荷变量。

**回归测试**：`tag1/hc-tools/tests/k3_checker.rs` `self_check_completes_on_stage1_sources`（13→14 项）——checker 对 stage1 三源完整跑完（exit 0、无解释器级 `error.*` 中止）。

**已知余量（K3.5，误报非崩溃）**：checker 的类/方法/字段建模不全（`parse_method`/`parse_field` 丢弃节点、`check_decl` 无 Class 分支、字段名被当标识符检查、`Self`/字段解析缺失），自检 lexer/parser/自身分别产生 690/1616/2387 行误报。收敛任务归入 K4 计划前置任务（见 `docs/SPEC/phase4/05-k4-execution-plan.md`）。