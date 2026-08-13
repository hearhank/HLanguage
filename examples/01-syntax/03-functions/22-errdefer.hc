// 22-errdefer.hc — 错误路径清理（12.18）
//
//   - defer：作用域退出始终执行
//   - errdefer：仅错误返回路径执行（Zig 式）
//   - 场景：写入中途出错 → 回滚/清理

fn write_config(io: Io, path: &[u8], data: &[u8]) !void {
    var f = try io.fs.open(path);
    defer f.close();                    // 正常/错误都关闭

    var tmp = try io.fs.open("tmp.tmp");
    defer tmp.close();
    errdefer io.fs.remove("tmp.tmp");   // 仅出错时：清理临时文件

    try tmp.write_all(data);
    try io.fs.rename("tmp.tmp", path);  // 成功：原子替换
}

fn main(io: Io) !void {
    try write_config(io, "config.json", "{}");
    io.print("written\n");
}
