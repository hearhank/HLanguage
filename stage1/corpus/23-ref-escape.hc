// 23-ref-escape.hc — 引用逃逸检测（返回局部变量引用）
fn make_ref() *i32 {
    var x: i32 = 42;
    return &x;
}
