# Struct 类型与特性系统（2026-08-24）

## 状态

已定案（2026-08-24 grilling 会话，Q1–Q27 全部裁决）

## 背景

H 语言当前使用 `class` 作为统一类型定义关键字，通过 `[continuous]` 特性标注区分连续内存值类型与堆上引用类型。需要增加 `struct` 类型，使其天然为连续内存值类型，并建立统一的特性（attribute）系统。

## 决策

### 1. struct 与 class 共存

- **`struct`**：新增关键字，天然连续内存值类型（不需要 `[continuous]` 标注），栈分配
- **`class`**：保留关键字，堆上引用类型（默认），`[continuous]` 特性被删除
- struct 字段类型限制：仅标量、定长标量数组、嵌套连续 struct（见 Q3）

### 2. 内存布局

- 与 C ABI 兼容（字段顺序、padding 规则一致），方便 FFI 零拷贝互操作（Q1）
- `@sizeOf`/`@offsetOf` 行为与 C 一致

### 3. 分配模型

- **栈分配**：`let p = Point{x = 1, y = 2.1};`（普通局部变量，生命周期与作用域绑定）
- **堆分配**：`let p = alloc.init(Point);` 或 `let p = alloc.init(Point{x = 1, y = 2.1});` — 声明时直接分配到堆
- **装箱**：`let p = Point{x = 1, y = 2.1}; let boxed = box p;` — 栈变量装箱到堆，返回指针类型，自动释放（RAII），所有权绑定到当前作用域（Q7/Q14）
- 分配器：默认全局页分配器，暂不支持自定义分配器（Q16）

### 4. 对齐特性

- **语法**：`[Align(n)] struct ABC { a: i32, b: f32 }`（Q2，选项 B）
- **n 的取值范围**：1, 2, 4, 8（常见 CPU 对齐值，Q11）
- **字段级对齐**：支持 `[Align(8)] a: i32`（Q12）
- **语义**：控制 struct 末尾对齐要求（C ABI 兼容，Q19）
- 默认对齐 = 字段类型的自然对齐（C ABI 规则）

### 5. 特性系统

- 特性是编译时 struct 实例，用中括号包裹（Q5）
- 语法：`[Test]` / `[Test{timeout=30}]` / `[Test(timeout=30)]`（单参简写）
- 特性参数支持编译期常量表达式：数字、字符串、枚举（Q25）
- 特性编译后完全擦除，二进制不可见（Q20）
- `[test()]` 和 `[test(timeout)]` 转换为 `test{type=TestType.Timeout}` 形式

### 6. IAttribute 接口

- 系统增加 `IAttribute` 接口，用于标明 struct 是内部特性（Q23）
- 系统特性（编译器内置处理）vs 用户特性（编译器扩展处理，暂不做）
- 编译器内部以特性类型注册成字典，通过字典查找处理方法（Q24）

### 7. 编译器插件架构

- 使用 `@import` 加载插件（Q6，方案 B）
- 插件在语义分析阶段遍历 AST 中标记了相应特性的节点并转换/验证（Q6，方案 C）
- 第一阶段：编译器内置处理，插件 API 暂不暴露（Q24，方案 C）
- 插件可以：验证、转换、生成代码（Q26）

### 8. 扩展方法

- 语法：`[Extension(Point)] fn distance(self: *Point, other: *Point) f32 { ... }`（Q15，方案 C）
- 不能访问私有字段（Q15）
- 暂缓实现

### 9. 字段默认值

- 支持：`struct ABC { a: i32 = 0, b: f32 = 1.0 }`（Q13）
- `ABC{}` 使用默认值初始化

### 10. 删除的内容

- `[continuous]` 特性被删除（struct 天然连续，Q21/Q22）
- `struct` 关键字不再映射到 `KwClass`（改为 `KwStruct`）

## 影响

- **Lexer**：新增 `KwStruct` token；`struct` 不再映射到 `KwClass`
- **AST**：新增 `Decl::Struct` 变体；`Trait::Continuous` 删除；`Trait::Align` 改为存储对齐数值
- **Parser**：新增 `parse_struct` 方法；`[Align(n)]` 接受数字而非类型名
- **Semantic**：新增 struct 类型检查和布局计算
- **stage1**：同步更新（H 版 lexer/parser）
- **SPEC**：更新 `06-03-extended-types.md` 和 `00-feature-inventory.md`