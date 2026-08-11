// 纯块子集：编译后端验证（双后端一致性）
// h run examples/calc.h 与 h build examples/calc.h --exec 输出必须完全一致

struct Point {
    x: f64
    y: f64
}

enum Shape { Circle, Square, Triangle }

fun area(shape: Shape, size: f64) -> f64 {
    return match shape {
        Circle => 3.14159 * size * size
        Square => size * size
        Triangle => 0.5 * size * size
    }
}

fun distance(p: Point, q: Point) -> f64 {
    dx = p.x - q.x
    dy = p.y - q.y
    return dx * dx + dy * dy
}

fun main() -> void {
    print("圆面积:", area(Shape.Circle, 2.0))
    print("方面积:", area(Shape.Square, 3.0))
    print("三角面积:", area(Shape.Triangle, 4.0))
    p = Point{ x: 0.0, y: 0.0 }
    q = Point{ x: 3.0, y: 4.0 }
    print("距离平方:", distance(p, q))
    s = Shape.Triangle
    if s == Shape.Triangle {
        print("枚举比较: 是三角形")
    }
}
