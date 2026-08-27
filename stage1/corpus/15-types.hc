// 15-types.hc — 类型声明（class / enum / union / interface）
class Point {
    x: i32,
    y: i32,
}
enum Color {
    Red,
    Green,
    Blue,
}
union Data {
    i: i32,
    f: f64,
}
interface Iterable {
    fn next() ?i32;
}
fn main() void {
    var p: Point = Point{x = 1, y = 2};
}