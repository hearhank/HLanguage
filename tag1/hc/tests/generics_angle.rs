//! 临时验证：泛型 <> 语法迁移
use hc::parse_source;

fn try_parse(label: &str, src: &str) {
    match parse_source(src) {
        Ok(_) => println!("OK   {label}"),
        Err(diags) => {
            println!("FAIL {label}");
            for d in diags {
                println!("  {:?}: {}", d.span, d.message);
            }
        }
    }
}

#[test]
fn generics_angle_parse() {
    // 类型位置 Vec<i32>
    try_parse(
        "Vec type position",
        "fn f(a: Vec<i32>) void {}\nfn main() !void {}\n",
    );
    // 嵌套 Vec<Vec<i32>>
    try_parse(
        "nested",
        "fn f(a: Vec<Vec<i32>>) void {}\nfn main() !void {}\n",
    );
    // 泛型函数 fn swap<T>
    try_parse(
        "generic fn <T>",
        "fn swap<T>(a: *mut T, b: *mut T) void {}\nfn main() !void {}\n",
    );
    // 泛型函数 fn sum<T> where
    try_parse(
        "generic fn <T> where",
        "fn sum<T>(items: &[T]) T where T: INumber { return items[0]; }\n",
    );
    // 表达式位置 Vec<i32>.init(alloc)
    try_parse(
        "expr Vec<i32>.init",
        "fn main() !void { var v = Vec<i32>.init(alloc); }\n",
    );
    // 泛型字面量 Pair<i32>{...}
    try_parse(
        "Pair<i32> literal",
        "fn main() !void { var p: Pair<i32> = Pair<i32>{first = 1, second = 2}; }\n",
    );
    // 比较运算符仍正常
    try_parse(
        "comparison",
        "fn cmp(a: i32, b: i32) bool { return a < b and b <= 3; }\n",
    );
    // FnN
    try_parse(
        "Fn1<i32> i32",
        "fn apply(f: Fn1<i32> i32, x: i32) i32 { return f(x); }\n",
    );
    // 三重嵌套 Vec<Vec<Vec<i32>>>
    try_parse(
        "triple nested",
        "fn f(a: Vec<Vec<Vec<i32>>>) void {}\nfn main() !void {}\n",
    );
    // Map<&[u8], Vec<i32>> 混合嵌套
    try_parse(
        "Map mixed nested",
        "fn f(m: Map<&[u8], Vec<i32>>) void {}\n",
    );
    // 多类型参数 fn swap<T, U>
    try_parse("multi type params", "fn pair<T, U>(a: T, b: U) void {}\n");
    // where 子句 + 泛型方法（class）
    try_parse(
        "generic method",
        "class C { fn save<T>(self: *Self, io: *T) !void where T: Io {} }\n",
    );
    // 接口泛型方法
    try_parse(
        "interface generic method",
        "interface I { fn save<T>(self: *Self, io: *T) !void where T: Io; }\n",
    );
    // Vec<&[u8]>.init 表达式
    try_parse(
        "Vec<&[u8]>.init",
        "fn main() !void { var v = Vec<&[u8]>.init(alloc); }\n",
    );
}
