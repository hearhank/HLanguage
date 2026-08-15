use hc_rt::Interp;

fn load(src: &str) -> Interp {
    let program = hc::parse_source(src).unwrap();
    let mut interp = Interp::new(src);
    interp.load(&program).unwrap();
    interp
}

#[test]
fn dbg_fib_only() {
    let src = "fn fib(n: i32) i32 {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
test fn t() !void {
    try expect_eq(fib(10), 55);
}\n";
    let mut interp = load(src);
    eprintln!("loaded");
    let (p, f, _) = interp.run_tests();
    eprintln!("fib run: {p} {f}");
}

#[test]
fn dbg_tree_only() {
    let src = "tree Node {
    value: i32,
    children: Vec(Node),
    fn depth(self: *Self) i32 {
        var max_depth = 0;
        for (self.children) |child| {
            var d = child.depth();
            if (d > max_depth) { max_depth = d; }
        }
        return max_depth + 1;
    }
}
test fn t() !void {
    var root: o Node = Node.new(1, alloc);
    var child: o Node = Node.new(2, alloc);
    child.children.append(Node.new(3, alloc));
    root.children.append(move child);
    try expect_eq(root.depth(), 3);
}\n";
    let mut interp = load(src);
    eprintln!("loaded");
    let (p, f, _) = interp.run_tests();
    eprintln!("tree run: {p} {f}");
}
