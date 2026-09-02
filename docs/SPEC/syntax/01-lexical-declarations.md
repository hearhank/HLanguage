# 01 词法与声明

> 大模块：词法与声明 | 对齐状态：**✅ 对齐完成（2026-08-30，6 项裁决落定）** | 初稿：2026-08-30
>
> 事实基础：ADR-0005（所有权语法及修订）、ADR-0014（K1 union / K5 export）、ADR-0020（extern）、`docs/SPEC/phase1/12-script-redesign.md`（script 移除）、tag1 实现（`tag1/hc/src/lexer/`、`tag1/hc/src/parser/`）。
> 历史来源：`docs/SPEC/phase1/06-01-syntax.md`（已废弃，差异见文末变更记录）。
> 证据总库：`tag1/hc/tests/frontend.rs`（前端）、`tag1/hc/tests/owned_check.rs`（owned）。

## 1.1 词法基础

### 1.1.1 源文件与编码

- 规则：源码后缀 `.hc`（脚本文件 `.hs`，见 `10-meta.md`）；UTF-8 编码；位置信息（行/列）1 基。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/token.rs`（`Span`）

### 1.1.2 标识符

- 规则：以字母（Unicode 字母）或 `_` 开头，后续为 Unicode 字母/数字/下划线；不能与关键字同名（关键字表见 §1.2）。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_ident_text`（`c.is_alphanumeric() || c == '_'`）

```hc
var count = 0;
var 数据 = 1;      // Unicode 标识符合法（实现按 Unicode 字母接受）
```

- **`_` 忽略绑定**（裁决 D7，2026-08-30）：现状——`_` 单独出现时词法上是普通标识符（`lexer/mod.rs` 首分支吞掉 `_`）；忽略绑定仅在元组解构中实现（`var (a, _) = t;`）。
- ⏳ **通用忽略绑定（目标）**：`_` 可作任意被忽略位置的绑定（如赋值左侧 `_ = f();`、参数位）——需改 lexer 与语义层，实现路线见 `00-index.md`。

## 1.2 关键字

### 1.2.1 关键字清单（冻结）

- 规则：以下 **42** 个词为关键字，不可作标识符。按组列出（与实现逐字核对；含已废弃但词法保留的 `script`）：

| 组 | 关键字 |
|---|---|
| 声明 | `var` `const` `fn` `global` |
| 控制流 | `if` `else` `while` `for` `break` `continue` `return` `switch` `defer` `errdefer` |
| 类型构造 | `class` `struct` `enum` `union` `interface` `where` |
| 模块 | `namespace` `import` `pub` `export` |
| 所有权 | `owned` `move` `mut` |
| 操作 | `try` `catch` `orelse` |
| 元编程 | `script` `comptime` `anytype` `type` |
| 并发 | `async` `await` `spawn` |
| 外部接口 | `extern` |
| 字面量 | `void` `null` `true` `false` |

> 修订（D9/D13，ADR-0037 2026-08-31）：`and`/`or` 移除（逻辑运算符 = `&&`/`\|\|`，Rust 语法为正）；`tree` 移除（连预留都不保留）。原 45 → 42。

- 状态：✅ 已实现（词法层；个别关键字的**声明级**行为见下）
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_ident_or_keyword`（42 项逐一对应）

**关键字级特殊行为**（声明层裁决，非词法层）：

| 关键字 | 声明级行为 | 状态 |
|---|---|---|
| `script` | `script { }` 块已移除（2026-08-23），声明位置出现即报错并指引导迁 `.hs` 文件；`.hs` 脚本系统的实现排期 = ⏸ 自举后（见 `00-index.md` 排除列表） | ✅ 诊断实现 |
| `struct` | 独立声明形态 `parse_struct`（与 `class` 并存，语义差异在 `04-extended-types.md` 裁决） | ✅ 可解析 |
| `union` | 无标签联合（ADR-0014 K1），无方法，成员可 `pub` | ✅ 可解析 |
| `export` | 仅修饰 `fn`/`async fn`（K5 原生符号导出，链接器可见）；修饰其它声明 → 诊断 "`export` only applies to `fn`/`async fn`" | ✅ |
| `extern` | `extern fn` 纯声明（无 body，`;` 结尾，链接期解析外部符号，ADR-0020 A1） | ✅ |
| `owned` | 仅作为**类型前缀**合法（§1.9）；`move` 仅作为**表达式前缀**合法（见 `07-ownership-memory.md`） | ✅ |
| `test` | **不是关键字**——测试 = `[test(...)]` 特性标注（`12-testing.md`） | ✅ |

### 1.2.2 逻辑运算符 `&&`/`\|\|`

- 规则：**`&&` = 逻辑与、`\|\|` = 逻辑或**（token `AndAnd`/`PipePipe`，Rust 语法为正——修订 D9，ADR-0037 2026-08-31）。`and`/`or` **不再是关键字**（变普通标识符）。
- `\|\|` 的另一身份：**错误集联合**仅存在于 `const E = A \|\| B;` 特例（`parse_const` 文本特判，先于表达式解析）与类型位置（归 `08-errors.md` 裁决）。
- `&&` 不是双引用——H 无 `&&T` 形态（取址的取址不支持；需要时单独立项）。
- 状态：✅ 已实现
- 证据：`lexer/mod.rs` `lex_punct`（`&&`→`AndAnd`、`\|\|`→`PipePipe`）；`parser/expr.rs` `parse_and`/`parse_or`

### 1.2.3 内建函数前缀 `@`

- 规则：`@` 后跟标识符 = 内建函数引用（`@sizeOf`、`@intCast`…），不与用户标识符冲突；`@` 后必须是标识符。内建全集见 `13-builtins.md`。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` L43-47（`AtBuiltin(name)`）

## 1.3 注释

- 规则：
  - `//` 行注释；`///` 文档注释——**词法上与行注释等价**，与声明的关联由文档生成器/LSP 处理（`///` 必须紧贴声明上方才有关联意义，该规则归 `11` 工具链，非词法）。
  - `/* */` 块注释，**支持嵌套**（`/*` 递增、`*/` 递减，计数配对闭合——修订 D10，ADR-0037 2026-08-31）。
  - 错误：未闭合块注释 → 诊断 `unterminated block comment`。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` `skip_ws_and_comments`（嵌套深度计数）；`stage1/lexer.hc` `skip_ws`（H 版同步）

```hc
// 行注释
/// 文档注释（关联下一声明）
/* 块注释 /* 现在可以嵌套 */ 里层闭合后外层继续 */
```

## 1.4 数字字面量

### 1.4.1 整数字面量

- 规则：
  - 十进制：`0-9` 组合；`_` 作分隔符（任意位置）。
  - **进制前缀**：`0x`/`0X`（十六进制）、`0b`/`0B`（二进制）、`0o`/`0O`（八进制）；前缀后跟对应进制数字与 `_`；**前缀后无有效数字位 → 词法诊断直接报错**（`invalid hex literal` / `invalid binary literal` / `invalid octal literal`——修订 D12，ADR-0037 2026-08-31；参考 Rust/Zig 词法报错，不再产出 `Int(0)` + Ident 的延迟错误）。
  - **宽度后缀（可选）**：`i8`…`i128`、`u8`…`u128`、`isize`、`usize`——如 `42i32`、`255u8`。带后缀 = 定宽字面量；不带 = comptime_int（使用处定型，超范围编译期报错，见 `03-types.md`）。
- 状态：✅ 已实现（宽度后缀为**旧文档漏记补录**）
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_number`（L187-238）+ `maybe_suffix`（L297-325）

```hc
var a = 1_000_000;      // comptime_int
var b = 0xFF;           // 255
var c = 0b1010_1010;
var d = 0o777;
var e = 42i32;          // 定宽 i32
var f = 255u8;
```

> ⚠️ 示例注记：本规范各模块示例中 `var a = 1_000_000;` 类**省略类型注解**的写法，依赖 ⏸ 自举后的变量类型推断（裁决 H1，2026-08-30，见 §1.9.1 / `03-types.md` §3.9）；示例仅演示字面量形态，规范代码应写 `var a: i32 = 1_000_000;`。

### 1.4.2 浮点字面量

- 规则：
  - 小数点后**必须有数字**（`1.` 不是浮点字面量——词法为 Int `1` + `.`）；`.` 前必须有数字（`.5` 不是浮点——词法为 `.` + Int）。
  - 指数：`e`/`E` + 可选 `+`/`-` + 数字（如 `1.5e10`、`2e-3`）。
  - 宽度后缀：`f32`/`f64`（如 `3.14f64`）；不带 = comptime_float。
- 状态：✅ 已实现（指数与后缀为旧文档漏记补录）
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_number` L240-295

```hc
var x = 3.14;       // comptime_float
var y = 1.5e-10;
var z = 2.0f64;
```

## 1.5 字符字面量

- 规则：`'x'` 单引号包裹**单个 Unicode 标量字符**——支持直接书写非 ASCII 字符（`'中'`）与 `\u{...}` 转义（`'\u{4E2D}'`）；转义集合 `\n` `\r` `\t` `\\` `\'` `\xNN` `\u{...}`；token 携带 **u32 码点**。未闭合 → `char literal must be closed with '`；空字面量/超一个标量 → 词法诊断。
- **定型**：字符字面量 = **comptime_int**（码点值；Zig 式），ASCII 场景行为不变（赋给 `u8` 零迁移）；非 ASCII 码点赋给 `u8` 超范围编译期报错，用 `i32` 或字符串表达。显式 `char` 类型 = 后续提案（修订 D11，ADR-0037 2026-08-31；取代 D1「单 ASCII 字节」限定）。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_char`（u32 码点 + `\u{...}`）；`semantic/infer.rs` `Expr::CharLit` → comptime_int；`stage1/lexer.hc` `lex_char`（H 版同步）

```hc
var nl = '\n';
var a  = 'A';        // comptime_int = 65；赋给 u8/i32 均可
var zh = '中';       // comptime_int = 0x4E2D（20013）
var esc = '\u{1F600}'; // 码点 128512
// 赋给 u8：'中' → 编译期报错（超 u8 范围）
```

## 1.6 字符串字面量

### 1.6.1 常规字符串

- 规则：`"..."`；转义集合：`\n` `\r` `\t` `\\` `\"` `\'` `\xNN`（十六进制字节）`\u{...}`（Unicode 码点 → UTF-8）；非法转义 → 诊断 `invalid escape sequence` / `invalid \x escape` / `invalid \u escape` / `\u escape out of range`；未闭合 → `unterminated string literal`。转义在**词法阶段解码**（token 携带已解码文本）。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_string`（L327-407）

```hc
var s = "Hello\n\u{1F600}\x41";
```

### 1.6.2 原始多行字符串

- 规则：`"""..."""` 包裹；内容**不处理任何转义**（反斜杠原样保留）；可跨行；以 `"""` 闭合；未闭合 → `unterminated raw string`。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` L329-352

```hc
var path = """C:\Users\raw\no_escape""";
var doc = """
  第一行
  第二行
""";
```

- 字符串的静态存储/只读切片语义（无 `o`、不可 move）归 `04-extended-types.md` §4.7（String = `&[u8]` 别名，G1 裁决），本条只管词法。

## 1.7 标点与运算符 token

- 规则：词法 token 全集（与实现逐项核对）：

| 类别 | token |
|---|---|
| 括号/分隔 | `{ } ( ) [ ] ; , . :` |
| 赋值/相等 | `=` `==` `!=` `=>` |
| 比较 | `<` `<=` `>` `>=` |
| 算术 | `+` `-` `*` `/` `%` `%%` |
| 复合赋值 | `+=` `-=` `*=` `/=` `&=` `\|=` `^=` |
| 位运算 | `&` `\|` `^` `~` `<<` `>>` |
| 其它 | `!` `?` `..` `.*` `\|\|` |

- 无 `->`（函数返回类型直接跟在 `)` 后，见 `05-functions.md`）；无 `::`（命名空间路径用 `.`，见 `09-modules-entry.md`）。
- 未预期字符 → 诊断 `unexpected character`。
- 状态：✅ 已实现（复合赋值 `&=` `\|=` `^=` 为旧文档漏记补录）
- 证据：`tag1/hc/src/lexer/mod.rs` `lex_punct`（L447-602）、`token.rs` `TokenKind`

## 1.8 声明总形态

- 规则：顶层/命名空间内声明的统一前缀次序：

```
[pub] [export] [特性标注...] 声明
```

- `pub`：跨包可见标志（默认私有，M7.2）。
- `export`：仅 `fn`/`async fn`（§1.2.1）。
- 特性标注（`[...]`）当前位置合法集：
  - `[continuous]` `[pad]` `[align(T)]` —— 仅 class/tree（详见 `04-extended-types.md`）
  - `[test(...)]` —— 仅 fn（详见 `12-testing.md`）
  - `[extension(T)]` —— 仅 fn（扩展方法，详见 `05-functions.md`）
- ❌ `[module]`：已移除（ADR-0026）——出现即报错，指引改用 `src/Modules/` 目录（诊断原文：`[module] is removed. Use src/Modules/ directory instead (see ADR-0026).`）。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/parser/decl.rs` `parse_decl`（L60-287）、`parse_trait`（L289-315）

## 1.9 变量声明 `var`

### 1.9.1 基本形态

- 规则：`var [mut] 名称 [: 类型] [= 初始化表达式];`
  - `mut`：可写修饰；**缺省 = 只读**（ADR-0005 R1 定案，Rust 标杆）。
  - **类型推断 ✅（修订 D8，ADR-0037 2026-08-31；取代 H1「注解必填」排期）**：类型注解可省略，从初始化表达式推断（`declared.or(init_ty)`）；**仅无法推断时报诊断**（无初始化器且无注解 → `cannot infer type; add an explicit type annotation`）。函数参数/返回类型仍必须显式（与 Rust 一致）；`global` 有初始化器可推断，无初始化器（零值）必须显式标注。
  - 初始化表达式在**语法层**可省略（先声明后赋值 = 需显式标注类型）。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/parser/stmt.rs` `parse_var_decl`（ty 可选）；`semantic/check.rs` check_stmt VarDecl（`var_ty = declared.or(init_ty)` + 无法推断诊断）；裁决 D8

```hc
var x: i32 = 5;        // 全写
var y = x;             // ✅ 推断（init 为 i32）
var mut z = 0;         // ✅ 推断 + 可写
var w;                 // ✋ 诊断：cannot infer type; add an explicit type annotation
var items = Vec<u8>.init(alloc);   // ✅ 推断为 Vec<u8>
```

### 1.9.2 所有权类型形态 `owned T`

- 规则：`owned` 是**类型前缀**，只能出现在类型位置：`owned T` / `owned *mut T`。**`owned` 后必须跟类型**——不存在「仅标 `owned` 不写类型」的形态。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/parser/mod.rs` `parse_type`（L69-75：`KwOwned → Type::Owned(inner)`）
- ❌ 旧文档形态作废：`var mut x: owned = t;`（「类型与 *mut 推断，o 显式」）——实现中 `owned` 后必须跟类型，该形态解析即报错。
- **裁决（D3，2026-08-30）**：该形态**删除**——ADR-0005 Q22b 签名制中不存在「仅标 `owned` 不写类型」的形态，规范不再收录；如未来需要，单独提案。

```hc
var mut p: owned *mut Person = alloc.init(Person);   // ✅ 全写
var mut q: owned *mut = alloc.init(Person);          // ✅ *mut 推断（owned *mut T）
// 错误示例：var mut r: owned = alloc.init(Person);
//   → 诊断：owned 后缺类型（解析失败）
```

- `o` 记号：❌ 已废弃（ADR-0005 Q22b 签名制取代；`ast.rs` 中 `Type::Owned` 的调试显示仍是 `o T` 属实现内部遗留，非语法）。
- 语义规则（所有权注册/销毁/move 资格）归 `07-ownership-memory.md`。

### 1.9.3 元组解构声明

- 规则：`var [mut] (名称, 名称, ...) = 表达式;`；元素位可用 `_` 忽略；右侧为元组/多值返回。
- 状态：✅ 前期目标达成（D14，ADR-0037 2026-08-31）：**元组解构 + 函数多返回值解构**可用；全场景（赋值左侧、参数位、match 等）保留 ⏳。D6 的「解构不支持 `mut`」不变。
- 证据：`tag1/hc/src/parser/stmt.rs` `parse_var_decl` 元组分支（`_` 忽略位）；D14

```hc
var (a, b) = pair();      // a、b 分别绑定（含函数多返回值）
var (x, _) = pair();      // 忽略第二元素
var mut (m, n) = pair();  // ✋ 诊断：解构声明不支持 mut（D6）
```

## 1.10 常量声明 `const`

- 规则：`const [pub] 名称 [: 类型] = 表达式;`——初始化表达式**必填**（与 `var` 的关键差异）；常量不可变。声明级（namespace/顶层）与块级（语句位置）均合法。
- 命名约定：`SCREAMING_SNAKE`（设计规则 §0.8，约定非语法强制）。
- 错误集特例（语法层专门支持，详见 `08-errors.md`）：
  - `const E = error{成员1, 成员2};` —— 错误集字面量 → 注册为错误集类型别名
  - `const E = 集合A || 集合B;` —— 错误集联合 → 注册为联合错误集别名
- 状态：✅ 已实现
- 证据：`tag1/hc/src/parser/decl.rs` `parse_const`（L407-483）；块级：`tag1/hc/src/parser/stmt.rs` `parse_stmt`（KwConst 分支）

```hc
const MAX = 100;
const NAME: String = "h";
const ParseErr = error{invalid_char, out_of_range};   // 错误集
const AnyErr = ParseErr || IoErr;                     // 错误集联合
```

## 1.11 全局声明 `global`

- 规则：`global [pub] 名称 [: 类型] [= 初始化表达式];`——仅声明级（块内无 `global`）；静态生命周期（程序退出时销毁）；所有权归根作用域，不可 move（ADR-0005）；初始化在 main 前执行，跨文件按依赖图拓扑排序、循环依赖 = 编译错误（M7 定案）。
- 状态：⚠️ 部分实现（语法 ✅；初始化拓扑排序与销毁语义见 `07-ownership-memory.md`）
- 证据：`tag1/hc/src/parser/decl.rs` `parse_global`（L383-405）
- **裁决（D4，2026-08-30）**：允许无初始化器 = **零值初始化**（Zig 式）；零值定义与类型约束的语义规则由 `03-types.md` 定义，零值初始化语义实现/验证 → 记入 `00-index.md` 实现待对齐清单 #2。

## 1.12 `void` 与字面量关键字

- 规则：`void` 仅作函数返回类型（「无值」类型，见 `03-types.md`）；`null` / `true` / `false` 为字面量关键字；`true`/`false` 类型 = `bool`。
- 状态：✅ 已实现
- 证据：`tag1/hc/src/lexer/mod.rs` L166-169

## 1.13 变更记录（相对旧 06-01-syntax.md）

| 变更 | 依据 |
|---|---|
| `o` 从关键字表删除；所有权标注统一为类型前缀 `owned T` | ADR-0005 Q22b + 实现（实现从未有过 `o` 关键字） |
| `test` 移出关键字表（测试 = `[test(...)]` 特性标注） | 实现一致（`parse_test_trait`） |
| 关键字表补录 `struct` `union` `export` `extern`（实现一直存在，旧文档漏记） | ADR-0014 K1/K5、ADR-0020 A1 + 实现 |
| `script { }` 块标 ❌（保留关键字 + 声明级报错指引 `.hs`） | 12-script-redesign.md（2026-08-23） |
| `[module]` 标 ❌（报错指引 `src/Modules/`） | ADR-0026 |
| 数字字面量补录宽度后缀（`iN`/`uN`/`fN`/`isize`/`usize`）与浮点指数 | 实现 `maybe_suffix`/`lex_number` |
| 复合赋值补录 `&=` `\|=` `^=` | 实现 `lex_punct` |
| 补录 `&&` = `and` 别名、`\|\|` = 逻辑或别名（E1 裁决：错误集联合仅限 const 特例） | 实现 L560-563、`expr.rs` L11 |
| 字符字面量明确为单 ASCII 字节（旧「comptime_int 惰性宽度」表述待裁决 D1） | 实现 `lex_char` |
| 删除 `var mut x: owned = t;` 形态（待裁决 D3 是否保留为目标） | 实现 `parse_type` |
| 块注释明确不嵌套；`///` 明确与行注释词法等价 | 实现 `skip_ws_and_comments` |
| 元组解构、`_` 忽略绑定归入变量声明小节（旧散落在 06-总纲速查） | 禁止双写原则 |
| 裁决落定：D1 字符 = 单字节 / D2 `&&` 兼容别名 / D3 删除 `owned` 全推断形态 / D4 `global` 零值初始化 / D6 解构只读无 `mut` / D7 `_` 通用化列为 ⏳ | 项目所有者裁决（2026-08-30，见 §1.14） |
| **ADR-0037（2026-08-31）八项修订**：D8 局部推断为正（取代 H1 排期） / D9 逻辑运算符 Rust 化（`&&`/`\|\|` 为本体，`and`/`or` 移除，取代 D2） / D10 块注释嵌套 / D11 字符字面量 Unicode 码点化（修订 D1） / D12 数字空前缀词法报错 / D13 `tree` 关键字移除（取代 G2） / D14 解构前期范围定案 / D15 f32 真实宽度 + `f16` 移除（修订 F1，实现专项） | ADR-0037 + 项目所有者裁决（2026-08-31，见 §1.14） |

## 1.14 裁决记录（2026-08-30，项目所有者）

| # | 条目 | 裁决 | 影响 |
|---|---|---|---|
| D1 | 字符字面量语义类型 | ~~按实现——单 ASCII 字节（u8）~~ **修订 D11（ADR-0037）**：Unicode 标量码点（token u32），定型维持 comptime_int | §1.5、`03-types.md` |
| D2 | `&&` 别名定位 | ~~兼容别名；规范风格用 `and`~~ **修订 D9（ADR-0037）**：`&&`/`\|\|` 为本体，`and`/`or` 移除 | §1.2.2 |
| D3 | `var mut x: owned = t;` 形态 | **删除** | §1.9.2 |
| D4 | `global` 无初始化器 | **允许**——零值初始化 | §1.11、`03-types.md`、backlog #2 |
| D6 | 解构声明的 `mut` | **不支持 `mut`**——元组命名、元组只读；出现即报诊断 | §1.9.3、backlog #1 |
| D7 | `_` 通用忽略绑定 | **扩展为 ⏳ 目标**（前期：解构内 `_` ✅） | §1.1.2 |
| D8–D15 | 词法与声明八项修订 | 见 ADR-0037（2026-08-31）：推断 / `&&`/`\|\|` / 嵌套注释 / 字符码点 / 数字报错 / `tree` 移除 / 解构范围 / f32 宽度 | 全文 |
