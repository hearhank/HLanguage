// 52-string-deep.hc — String 方法集（值语义，Q16/Q28）
//
//   - 拼接 concat（Q28 无运算符）；比较 == 内容（Q16）
//   - split / join / find / substring / replace

fn main(io: Io) !void {
    var csv = String.from("a,b,c,d", alloc);

    // split：按分隔符切分 → Vec(String)
    var parts = csv.split(',');
    io.print("parts = {}\n", parts.len);

    // join：拼接（String 方法）
    var joined = String.join(&parts, " | ");
    io.print("{}\n", joined);

    // find：子串位置（?usize）
    var text = String.from("hello world", alloc);
    var found = text.find("world");
    io.print("pos = {}\n", found orelse -1);

    // substring / replace
    var sub = text.substring(0, 5);            // "hello"
    io.print("{}\n", sub);

    var replaced = text.replace("world", "h");
    io.print("{}\n", replaced);
}
