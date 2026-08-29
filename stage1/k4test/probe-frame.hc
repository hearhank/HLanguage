// probe-frame.hc — 复刻：局部持有 class 引用（含 Vec<Class> 字段）跨嵌套 self 调用是否稳定
class N {
    tag: i32,
    kids: Vec<N>,
}

class Host {
    root: N,
    mut touched: i32,

    fn touch(self: *mut Self) void {
        self.touched = self.touched + 1;
    }

    fn touch_with_vec(self: *mut Self) void {
        var t = Vec<i32>.init(alloc);
        t.append(1);
        self.touched = self.touched + t.len;
    }

    fn probe(self: *mut Self) i32 {
        var r = self.root;
        var a = r.kids[0].tag;
        self.touch();
        var b = r.kids[0].tag;
        self.touch_with_vec();
        var c = r.kids[0].tag;
        return a * 100 + b * 10 + c;
    }
}

fn make_tree() N {
    var leaf = N{tag = 7, kids = Vec<N>.init(alloc)};
    var root = N{tag = 0, kids = Vec<N>.init(alloc)};
    root.kids.append(leaf);
    return root;
}

fn main() !void {
    var h: Host = alloc.init(Host{
        root = make_tree(),
        touched = 0,
    });
    io.print("result={}\n", h.probe());
}
