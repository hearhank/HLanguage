// 31-class.hc — class/tree 复杂类型（12.20）
//
// Q22 定案（2026-08-13）：class 成员与 struct 完全一致
//   - 差异仅在：堆上存储、可 o、字段可带 o 子对象、脚本生成 new 构造样板
//   - 无构造块、字段规则统一（默认只读 / mut 修饰）
//   - tree = 递归/层级组合（父 o 拥有子，Q16 默认所有权）
// Q1–Q3 定案（2026-08-13）：可移动性谓词——
//   Counter（纯标量字段）与 Node（children: Vec(Node) 为自有子对象）均可 move；
//   含引用值字段（*T/*mut T/&[T]/&mut [T]）的 class 不可 move。

class Counter {
    mut count: i32,

    fn inc(self: *mut Self) void {
        self.count += 1;
    }

    fn get(self: *Self) i32 {
        return self.count;
    }
}

tree Node {                              // 递归组合
    value: i32,
    children: Vec(Node),                 // 组合：子节点

    fn total(self: *Self) i32 {
        var sum = self.value;
        for (self.children) |child| {
            sum += child.total();
        }
        return sum;
    }
}

fn main(io: Io) !void {
    // class：堆上、默认拥有（非 arena 分配器，Q16）；构造 = new 样板（脚本生成）
    var c: o Counter = Counter.new();
    c.inc();
    io.print("{}\n", c.get());

    // tree：递归组合
    var root: o Node = Node.new(1, alloc);
    root.children.append(Node.new(2, alloc));
    root.children.append(Node.new(3, alloc));
    io.print("total = {}\n", root.total());
}

test "class 方法与状态" {
    var c: o Counter = Counter.new();
    c.inc();
    c.inc();
    try expect_eq(c.get(), 2);
}

test "tree 递归组合" {
    var root: o Node = Node.new(1, alloc);
    root.children.append(Node.new(2, alloc));
    root.children.append(Node.new(3, alloc));
    try expect_eq(root.total(), 6);
}
