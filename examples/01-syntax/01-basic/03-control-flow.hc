import H.std.{io};

// 03-control-flow.hc — 控制流：if / while / for / defer / try
//
// Q4 定案（2026-08-13）：数组字面量
//   - 推断式：[1, 2, 3, 4, 5]（类型与长度自动推断，字面量惰性宽度）
//   - 显式式：[5]i32{1, 2, 3, 4, 5}
//
// 可写捕获：|mut item|（Rust 标杆，与 var mut 一致）

fn main() !void {
    // if 是表达式：作表达式时 else 强制
    var mut x: i32 = 7;
    var label: &[u8] = if (x > 5) "big" else "small";
    io.print("{}\n", label);

    // while + 续步表达式
    var mut i: i32 = 0;
    while (i < 5) : (i += 1) {
        io.print("i = {}\n", i);
    }

    // for 迭代（捕获默认只读）
    var arr = [1, 2, 3, 4, 5];
    for (arr) |item| {
        io.print("{}\n", item);
    }

    // for 可写捕获：修改元素
    var mut arr2 = [10, 20, 30];
    for (arr2) |mut item| {
        item *= 2;
    }

    // defer：作用域退出执行（资源清理，12.18）
    var f = try io.fs.open("data.txt");
    defer f.close();

    // try：error union 传播（错误在入口由运行时报告）
    var data: &[u8] = try io.fs.read_all(f, io.alloc);
    io.print("read {} bytes\n", data.len);
}

[test] fn if_expression() !void {
    var x: i32 = 7;
    var label: &[u8] = if (x > 5) "big" else "small";
    try expect_eq_slices(label, "big");
}

[test] fn while_continue_step() !void {
    var mut i: i32 = 0;
    var sum = 0;
    while (i < 5) : (i += 1) {
        sum += i;
    }
    try expect_eq(sum, 10);
}

[test] fn for_mutable_capture() !void {
    var mut arr2 = [10, 20, 30];
    for (arr2) |mut item| {
        item *= 2;
    }
    try expect_eq(arr2[0] + arr2[1] + arr2[2], 120);
}
