import H.std.{io};

// 66-builder-chain.hc — 方法链式调用（builder 模式）
//
//   - 方法返回 self（*mut Self）→ 链式调用
//   - 与「方法调用双语」（Q5）一致；接收者自动取引用

class Query {
    mut where_clause: String,
    mut limit_n: i32,

    fn where(self: *mut Self, cond: &[u8]) *mut Self {
        self.where_clause = String.from(cond, alloc);
        return self;
    }

    fn limit(self: *mut Self, n: i32) *mut Self {
        self.limit_n = n;
        return self;
    }
}

fn main() !void {
    var mut q: o Query = alloc.init(Query);   // 无参构造（C1'）

    // 链式调用：where().limit()（*mut 链不复制——资格随链传递）
    q.where("age > 18").limit(10);

    io.print("limit = {}\n", q.limit_n);
}

[test] fn builder_chaining() !void {
    var mut q: o Query = alloc.init(Query);
    q.where("age > 18").limit(10);   // 链：*mut 资格延续（Q25）
    try expect_eq_slices(q.where_clause.as_slice(), "age > 18");
    try expect_eq(q.limit_n, 10);
}
