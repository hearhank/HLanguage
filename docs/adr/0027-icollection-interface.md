# ADR-0027: ICollection 接口 — 集合类型统一分派

**日期**: 2026-08-26  
**状态**: 已采纳  
**基于**: Grilling session (Q1-Q15)

---

## 背景

H 语言中多个集合类型共享相同的方法签名：

| 方法 | Vec | Arr | Deque | 语义 |
|------|-----|-----|-------|------|
| `append(item)` | ✅ | ✅ | ✅ | 追加元素 |
| `len()` | ✅ | ✅ | ✅ | 元素数量 |
| `get(i)` | ✅ | ✅ | ✅ | 索引读取 |
| `put(i, v)` | ✅ | ✅ | ✅ | 索引写入 |
| `front()` | ✅ | ✅ | ✅ | 首元素 |
| `back()` | ✅ | ✅ | ✅ | 末元素 |

当前实现中，LLVM codegen 通过 `call_method` 中的运行时 tag 检查（`is_arr`/`is_str`/`is_cls`）和手工枚举的方法列表（`is_coll_method`）来分派这些方法。这导致：

1. **Vec 方法分派失败**：Vec 是 Class(tag 10) 不是 Arr(tag 8)，`is_arr` 检查失败，落入 Class strcmp 链，因 `Vec.append` 不在 `canon` 表中而 abort
2. **方法列表硬编码**：`is_coll_method` 手工枚举 `append|push_back|append_u64|extend|init|len|front|back|get|put`，新增方法需修改 LLVM codegen
3. **无法复用**：`extend`/`is_empty` 等基于 `append`/`len` 的方法需要在每个类型中重复实现

## 决策

### 引入 `ICollection<T>` 接口

```hc
interface ICollection<T> {
    fn append(self: *mut Self, item: T) void;
    fn len(self: &Self) usize;
    fn get(self: &Self, i: usize) ?T;
    fn put(self: *mut Self, i: usize, item: T) void;
    fn front(self: &Self) ?T;
    fn back(self: &Self) ?T;
}
```

### 类型实现接口

```hc
class Vec<T> : ICollection<T> {
    fn append(self: *mut Self, item: T) void { ... }
    fn len(self: &Self) usize { ... }
    // ...
}
```

### 编译期静态分派

- **无运行时虚表**：H 语言是静态类型，调用点已知具体类型
- **IR lowerer 负责分派**：语义分析阶段建立 `type_implements` 映射表，lowerer 查表后将 `v.append(x)` 转为 `CallBuiltin("Vec.append", ...)`
- **LLVM codegen 接收具体调用**：`call_builtin` 中已有 `append`/`len`/`get`/`put` 等 handler，只需确保名字匹配

### 移除运行时 tag 检查

- 移除 `call_method` 中的 `is_coll_method` 手工枚举
- 移除 `is_arr`/`is_str` 的运行时 tag 分支
- 集合方法统一走 `CallBuiltin` 路径

## 不纳入本次范围

- **类方法**（`Lexer.bump`/`Parser.advance`）：这些是特定类的方法，不跨类型共享，不需要接口
- **ICollection 以外的接口**：`IComparable`/`IHashable` 等后续添加

## 实现计划

| 任务 | 内容 | 预估 |
|------|------|------|
| T1 | 语义分析：建立 `type_implements` 映射表 | 1h |
| T2 | IR lowerer：接口方法查找，转为 `CallBuiltin` | 1h |
| T3 | 移除 `call_method` 中 `is_coll_method` 手工枚举 | 0.5h |
| T4 | 确保 `call_builtin` 中 Vec/Arr/Deque 的 append/len/get/put 都能正确处理 | 0.5h |
| T5 | 添加 `ICollection<T>` 接口定义到标准库 | 0.5h |
| T6 | 为 Vec/Arr/Deque 添加 `: ICollection<T>` 声明 | 0.5h |
| T7 | 测试：所有示例测试通过 + native test 通过 | 0.5h |

## 后果

- ✅ 集合方法分派从运行时 tag 检查改为编译期接口查找，消除 `NoMethod` 错误
- ✅ 新增集合类型只需实现 `ICollection<T>`，无需修改 LLVM codegen
- ✅ 为后续默认方法实现奠定基础
- ⚠️ 需要在语义分析阶段增加接口实现关系跟踪
- ⚠️ IR lowerer 需要查找接口方法映射