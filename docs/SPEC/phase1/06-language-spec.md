> [!WARNING] **已废弃（2026-08-30，ADR-0034）**——本文件为历史资料，不作为实现依据。现行语法权威依据：[docs/SPEC/syntax/00-index.md](../../syntax/00-index.md)
# H 语言规范（总纲）

> **语言规范初稿（Draft）**——语法参考，实作依据（M9 正式规范的前置）。按实现计划（`07-bootstrap-plan.md`）分主题拆分为独立文件（见下方索引）；术语定义以 `CONTEXT.md` 为准；设计共识见 `01-language-design.md`。

## 语言定位

H 是一门**以数据为中心**、同时支持**系统编程与脚本编程**的编程语言，源码后缀 `.hc`。核心哲学：定义数据、修改数据、传输数据、保存数据。同一份源码既可编译为原生二进制，也可作为脚本解释执行，两种模式**语义一致**（值语义、所有权语义、控制流一致；错误检测策略是独立维度——Debug 全检测、Release 裸路径、脚本模式 Debug 语义）。

## 双模式执行

- 架构：共享前端 → 共享 IR → 双后端（字节码 VM 脚本模式 + LLVM 原生编译模式）
- 入口：`fn main(io: Io) !void`——编译器注入 `io` 句柄（唯一例外；库函数 io 参数用虚拟类型制 `io: *T where T: Io`）
- 程序环境（2026-08-14；2026-08-17 修订）：`io.env(name) ?&[u8]` / `io.stdin`/`io.stdout`/`io.stderr`；**命令行参数经入口 `main(args)` 注入（0 号 = 程序名），`io.args()` 取消**
- 退出（2026-08-14）：**语言内建枚举（L3）** `enum ExitType { Exit, Error }` + `io.exit(t: ExitType, code: u8) !void`——`Exit` 正常静默 / `Error` 错误退出（打印错误标记）；`main` 返回 error → `Error`/1、正常 → `Exit`/0；测试失败 → 非零

## 语法速查

```hc
[continuous]
class Point { x: f32, y: f32, fn dist(a: *Point, b: *Point) f32 }  // 连续内存值类型（H1 特性标注）
class Person { name: String, age: i32 }                             // 未标注 → 堆上
var p = alloc.init(Person{ name = ..., age = 30 });                 // 带参构造（C1'：alloc.init(T{...})）
var q = alloc.init(Person);                                         // 无参构造（字段后续显式赋值）

enum Kind { player, enemy }                          // 合一式枚举
interface INumber: ICompare { fn add(self: *Self, other: Self) Self }  // 接口（冒号标注，可继承）
var t = (1, "a");                                    // 元组：访问 t.0 / 解构 var (a, b) = t;
var tbl = Table<i32>.init(alloc, 4, 8, 0);           // Table（M8：方法构造 + t[i, j]）
fn f(a: i32, b: i32 = 0) i32 { ... }                 // 重载 + 可选参数（尾部、编译期常量默认值）
[test] fn check() !void { try expect_eq(add(1, 2), 3); }  // 测试函数（[test("名称")] 特性标记）
script { ... }  /  comptime { ... }                   // 元编程双轨（第三块 E1 完整实现；最小集不实现）
```

## 文件索引

| 文件 | 主题 | 对应实现模块（07） |
|---|---|---|
| `06-01-syntax.md` | 词法、声明、运算符、语句与控制流、测试函数 | M1 前端 |
| `06-02-types.md` | 基础类型：标量、切片、可选、错误联合、指针 | M2 语义 |
| `06-03-extended-types.md` | 扩展类型：class（自动判定）、枚举、元组、Table、String、tree | M2 语义 |
| `06-04-functions.md` | 函数与闭包、内建函数（box/copy/@ 内建） | M2 语义 / M4 运行时 |
| `06-05-interfaces.md` | 接口、标量接口族（ICompare/INumber）、迭代契约、序列化内建 | M2 语义 / M4 内建 |
| `06-06-ownership.md` | 所有权与内存模型 | M2/M4 运行时 |
| `06-07-errors.md` | 错误处理（error union、错误码、@panic） | M2/M4 运行时 |
| `06-08-modules.md` | 模块与包（namespace/import/pub/build.zon） | M1 模块 |
| `06-09-meta.md` | 元编程（script / comptime） | 第三块 E1 |
| `06-10-concurrency.md` | 并发与异步（四模式、线程、Future） | 第三块 E2 |
| `06-13-project-structure.md` | 项目结构与代码管理约定（hc init / 源码测试依赖约定） | 第二部分组 H |

## 历史章节号迁移（2026-08-14 拆分）

旧 `06-language-spec.md` 单一文档的章节号在新结构中的对应（旧引用「06 §N」可按下表追溯）：

| 旧章节 | 新位置 |
|---|---|
| §1 词法 / §2 声明（变量） / §3 运算符 / §4 语句与控制流 / §2 测试函数 | `06-01-syntax.md` |
| §2 类型 / §2 接口（标量接口族） | `06-02-types.md` / `06-05-interfaces.md` |
| §2 类型定义 / 复杂类型 / 枚举 / String | `06-03-extended-types.md` |
| §5 函数与闭包 / 内建函数 / @ 内建 | `06-04-functions.md` |
| §2 接口 / 标量接口族 / 迭代 / 序列化 | `06-05-interfaces.md` |
| §6 所有权与内存 | `06-06-ownership.md` |
| §8 错误处理 | `06-07-errors.md` |
| §7 模块与包 | `06-08-modules.md` |
| §9 元编程 | `06-09-meta.md` |
| §10 并发与异步 | `06-10-concurrency.md` |
