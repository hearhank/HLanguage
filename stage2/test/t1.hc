// t1: 最小复现探针 — !void main + try 语句
fn main(args: Vec<String>) !void {
    try io.fs.write_file("stage2/test/t1.txt", "x", alloc);
    io.print("try ok\n");
}
