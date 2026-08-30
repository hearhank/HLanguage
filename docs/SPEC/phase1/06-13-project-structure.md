> [!WARNING] **已废弃（2026-08-30，ADR-0034）**——本文件为历史资料，不作为实现依据。现行语法权威依据：[docs/SPEC/syntax/00-index.md](../../syntax/00-index.md)
# H 语言规范：项目结构与代码管理约定

> 对应实现模块：07 第二块 M7 工具链（第二部分组 H「代码管理四项」）。本文档定义**项目代码结构**（目录骨架 / 源码与测试约定 / 依赖声明）与工具链命令约定；模块(domain) 约定见 `06-08-modules.md` §模块；`hc doc` 文档生成约定见 H4/H5。
>
> 已落地（2026-08-17，组 H1）：`hc init <name>` 脚手架 + 本约定；`hc pkg add --path` 与缺失依赖诊断（H2）、模块 domain 约定（H3）、`hc doc`（H4/H5）随后。

## 项目形态（目录 = 包）

- **目录 = 包（package）**：一个项目 = 一个目录，含 `build.zon` 清单 + `.hc` 源码文件（M1.4；`06-08-modules.md` §编译单元 / 文件模型）
- **入口约定**：应用包入口 = `main.hc`（目录运行/构建优先取 `main.hc`，否则目录内排序后首个 `.hc`——组 C1；`package_entry`）
- **build.zon**：包清单 = `const build = Build{ ... }` 数据字面量（Q26）——`name` / `version` / `kind` / `files` / `deps`

## 源码约定

- 源码 `.hc` 文件位于**包根**（与 build.zon 同目录）——同包文件**共享命名空间**（跨文件直接可见，Q21）
- 多文件按职责拆分（如 `main.hc` / `math.hc` / `io.hc`）；命名空间（`namespace X`）组织符号；`src/Modules/` 目录定义模块（隔离 + IoC 容器注入，见 `06-08-modules.md` 与 ADR-0026）
- **命名**：文件 `snake_case.hc`；类型/命名空间 `PascalCase`；函数/变量 `snake_case`；常量 `SCREAMING_SNAKE`——见 `01-language-design.md` §10
- 目录参数形态：`hc run <目录>` / `hc build <目录>` 把目录当包加载（入口 = `main.hc` 或首个 `.hc`）；单文件 `hc run file.hc` = 隐式单文件包

## 测试约定

- 测试 = `[test("名称")] fn`（Q-T1），**与源码同文件**（无独立 `test/` 目录）；`[test]` 函数可被普通代码调用/复用（Q-R11）
- `hc test <dir>` 递归收集 `.hc`（`study/` 设计草图目录除外）；按父目录分组（同目录 = 同包）
- 断言五件套（`expect` / `expect_eq` / `expect_neq` / `expect_error` / `expect_eq_slices`）测试函数内隐式可用；`[test]` 函数内隐式 `test_io` + `alloc`（Q-T4）
- `hc test --mode=compile` 原生交叉验证（Q-T5；需 zig cc）

## 依赖约定

- `build.zon` `deps = [ Pkg{ ... } ]`（Pkg 数组）；**本地依赖带 `path`**（相对路径，指向依赖包根）；无 path = 注册中心依赖（第三块 E5）
- `hc pkg add <name> --path <dir>` 写入依赖声明（H2）；缺失本地依赖在装载时**响亮诊断**（H2，不静默跳过）
- 依赖包 pub 符号以包名前缀登记，`import pkg.{sym}` 选择导入 / `pkg.sym(...)` 限定访问（ADR-0010；`06-08-modules.md` §导入语句）

## hc init 脚手架（H1 落地）

`hc init <name>` 在**当前目录**生成最小项目骨架：

```
<name>/
├── build.zon     # 清单：name/version/kind=Kind.exe/files=["main.hc"]/deps=[]
└── main.hc       # 入口 fn main(args: owned Vec(String)) !void + [test] 冒烟测试
```

- **名称校验**：`[A-Za-z0-9_-]`（目录名合法；非空、非 `.`/`..`）
- **安全**：目录已存在且非空 → 拒绝覆盖（报错退出，不触碰现有文件）
- 脚手架即**最小可运行示例**：`hc run <name>` / `hc test <name>` 全绿（CLI 测试保证）
- 骨架注释内嵌源码/测试/依赖约定；完整约定见本文件
