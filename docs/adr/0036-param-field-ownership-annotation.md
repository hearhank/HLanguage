# 参数与字段的所有权标注模型：owned 名称前缀 + mut T 必定拥有

2026-08-30 裁决（语法规范对齐会话，K1）。本 ADR 修订 ADR-0025 参数类型表（`mut T` 语义）并补充 ADR-0030 的参数形态规则。

## 决策

1. **`owned` 修饰符写在名称前**（参数与类型字段位置）：
   - 参数：`fn main(owned args: *mut Vec<String>) !void { }`
   - 字段：`class ABC { pub owned x: *mut T }` 或 `class ABC { pub owned x: mut T }`
   - var 声明保持类型前缀形态（`var x: owned T`，`01` §1.9.2）——两个位置两种语法。
2. **参数/字段位置的所有权语义按形态判定**：

| 形态 | 所有权 |
|---|---|
| `T` | 必定拥有（`*T` 类比同理） |
| `mut T` | 必定拥有（**修订 ADR-0025「mut T = 可写无所有权」**） |
| `*T` / `*mut T` | 不一定（借用）；`owned` 名称前缀显式标记拥有 → `owned *T` / `owned *mut T` |
| 值类型 + `owned` | **编译错误**（栈/连续类型无所有权，与 ADR-0025 规则 4 一致） |

3. **入口参数形态定案**：`fn main(owned args: *mut Vec<String>) !void`（取代 `owned Vec(String)`）。
4. AllocSource 门控不变（Arena 分配 / `global` 变量不可作拥有语义传参，ADR-0030 决策 4）。

## 影响

- Parser：参数/字段 `owned` 名称前缀；`mut T` 作为类型形态（当前 parse_type 不支持裸 `mut T`）；值类型 + owned 诊断；main 签名三后端同步。
- 语义：所有权判定由「形态 + AllocSource」共同决定；`owned T` 值形态参数废除令（ADR-0030）由本模型取代。
