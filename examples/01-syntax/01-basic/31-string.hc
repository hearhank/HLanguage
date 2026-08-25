//! String 值类型测试
//!
//! 验证值语义、复制、方法、生命周期

[Test] fn string_from_literal() !void {
    var s = String.from("hello");
    try expect_eq(s.as_slice().len, 5);
    try expect_eq_slices(s.as_slice(), "hello");
}

[Test] fn string_from_slice() !void {
    var buf: &[u8] = "world";
    var s = String.from_slice(buf);
    try expect_eq(s.as_slice().len, 5);
    try expect_eq_slices(s.as_slice(), "world");
}

[Test] fn string_from_int() !void {
    var s = String.from(42);
    try expect_eq_slices(s.as_slice(), "42");
}

[Test] fn string_concat() !void {
    var a = String.from("hello ");
    var b = String.from("world");
    var c = String.concat(a, b);
    try expect_eq_slices(c.as_slice(), "hello world");
}

[Test] fn string_compare_eq() !void {
    var a = String.from("abc");
    var b = String.from("abc");
    try expect_eq(String.compare(a, b), 0);
}

[Test] fn string_compare_lt() !void {
    var a = String.from("abc");
    var b = String.from("xyz");
    try expect_eq(String.compare(a, b), -1);
}

[Test] fn string_compare_gt() !void {
    var a = String.from("xyz");
    var b = String.from("abc");
    try expect_eq(String.compare(a, b), 1);
}

[Test] fn string_len() !void {
    var s = String.from("hello");
    try expect_eq(s.as_slice().len, 5);
}

[Test] fn string_copy_from() !void {
    var s1 = String.from("hello");
    var s2 = String.copy_from(s1);
    try expect_eq_slices(s2.as_slice(), "hello");
    try expect_eq(s1.as_slice().len, 5);
}

[Test] fn string_as_slice() !void {
    var s = String.from("test data");
    var slice = s.as_slice();
    try expect_eq_slices(slice, "test data");
}