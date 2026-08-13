// math.hc — 命名空间示例（Q21 定案，2026-08-13）
//
// 命名空间 = 逻辑分组（C# 式）：
//   - namespace 块可跨文件；一个文件可有多个命名空间
//   - pub 仅控制包边界（跨包可见需 pub + build.zon 依赖声明）
//   - 同包跨命名空间：using 引入即可

namespace Math {
    pub fn square(x: i32) i32 {
        return x * x;
    }

    fn helper(x: i32) i32 {
        return x + 1;   // 包内私有（无 pub）
    }
}
