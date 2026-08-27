import H.std.{io};

// 52-string-deep.hc — String 方法集（= u8[] 别名，Q3；concat 等返回新 String）
//
//   - 拼接 concat（Q28 无运算符）；比较 == 内容（Q16）
//   - split / join / find / substring / replace

fn main() !void {
    var csv = "a,b,c,d";

    // split：按分隔符切分 → Vec<String>
    var parts = csv.split(',');
    io.print("parts = {}\n", parts.len);

    // find：子串位置（?usize）
    var text = "hello world";
    var found = text.find("world");
    io.print("pos = {}\n", found orelse -1);

    // substring / replace
    var sub = text.substring(0, 5);            // "hello"
    io.print("{}\n", sub);

    var replaced = text.replace("world", "h");
    io.print("{}\n", replaced);
}

[Test] fn split_join() !void {
    var csv = "a,b,c,d";
    var parts = csv.split(',');
    try expect_eq(parts.len, 4);
}

[Test] fn find_substring_replace() !void {
    var text = "hello world";
    try expect_eq(text.find("world").?, 6);
    var sub = text.substring(0, 5);
    try expect_eq_slices(sub.as_slice(), "hello");
    var replaced = text.replace("world", "h");
    try expect_eq_slices(replaced.as_slice(), "hello h");
}
