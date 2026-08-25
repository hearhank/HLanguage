import H.std.{io};

// 46-recursion.hc — 递归与树算法
//
//   - 递归函数（fib）
//   - tree 复杂类型（12.20/31-class）+ 递归遍历

fn fib(n: i32) i32 {
    if (n < 2) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

tree Node {
    value: i32,
    children: Vec<Node>,

    fn depth(self: *Self) i32 {
        var max_depth = 0;
        for (self.children) |child| {
            var d = child.depth();
            if (d > max_depth) {
                max_depth = d;
            }
        }
        return max_depth + 1;
    }
}

fn main() !void {
    io.print("fib(10) = {}\n", fib(10));

    // tree 构建：Node.new 构造样板 + move 进 Vec（Q23 调用点显式 move）
    var root: Node = Node.new(1, alloc);
    var child: Node = Node.new(2, alloc);
    child.children.append(Node.new(3, alloc));
    root.children.append(move child);
    io.print("depth = {}\n", root.depth());
}

[Test] fn recursive_fib() !void {
    try expect_eq(fib(10), 55);
}

[Test] fn tree_depth() !void {
    var root: Node = Node.new(1, alloc);
    var child: Node = Node.new(2, alloc);
    child.children.append(Node.new(3, alloc));
    root.children.append(move child);
    try expect_eq(root.depth(), 3);
}
