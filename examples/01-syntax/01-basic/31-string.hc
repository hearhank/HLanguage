//! String 值类型测试
//!
//! 验证值语义、复制、方法、生命周期

[Test] fn string_type_annotation() !void {
    var s: String = "hello";
    try expect_eq(s.as_slice().len, 5);
    try expect_eq_slices(s.as_slice(), "hello");
}

[Test] fn string_from_literal() !void {
    var s = "hello";
    try expect_eq(s.as_slice().len, 5);
    try expect_eq_slices(s.as_slice(), "hello");
}

[Test] fn string_from_slice() !void {
    var buf: &[u8] = "world";
    try expect_eq(buf.len, 5);
    try expect_eq_slices(buf, "world");
}

[Test] fn string_from_int() !void {
    var s = String.fromInt(42);
    try expect_eq_slices(s.as_slice(), "42");
}

[Test] fn string_concat() !void {
    var a = "hello ";
    var b = "world";
    var c = a.concat(b);
    try expect_eq_slices(c.as_slice(), "hello world");
}

[Test] fn string_compare_eq() !void {
    var a = "abc";
    var b = "abc";
    try expect_eq(String.compare(a, b), 0);
}

[Test] fn string_compare_lt() !void {
    var a = "abc";
    var b = "xyz";
    try expect_eq(String.compare(a, b), -1);
}

[Test] fn string_compare_gt() !void {
    var a = "xyz";
    var b = "abc";
    try expect_eq(String.compare(a, b), 1);
}

[Test] fn string_len() !void {
    var s = "hello";
    try expect_eq(s.as_slice().len, 5);
}

[Test] fn string_copy_from() !void {
    var s1 = "hello";
    var s2 = s1;
    try expect_eq_slices(s2.as_slice(), "hello");
    try expect_eq(s1.as_slice().len, 5);
}

[Test] fn string_as_slice() !void {
    var s = "test data";
    var slice = s.as_slice();
    try expect_eq_slices(slice, "test data");
}