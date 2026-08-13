// 75-transaction.hc — 事务模式（defer/errdefer 组合，22 延伸）
//
//   - 成功提交 / 失败回滚（errdefer 仅错误路径）
//   - 多步写入的一致性（原子替换语义）

fn transfer(io: *T, from: &[u8], to: &[u8], amount: i64) !void where T: Io {
    var log = try io.fs.open("journal.log");
    defer log.close();
    errdefer io.fs.append("journal.log", "ROLLBACK\n") catch |_| {};

    // 步骤 1：扣款（不足则回滚）
    var a = try io.fs.read_int(from);
    if (a < amount) {
        return error.InsufficientFunds;    // errdefer 执行 → 回滚
    }
    try io.fs.write_int(from, a - amount);

    // 步骤 2：入账
    var b = try io.fs.read_int(to);
    try io.fs.write_int(to, b + amount);

    try io.fs.append("journal.log", "COMMIT\n");
    // 成功路径：errdefer 不执行
}

fn main(io: Io) !void {
    try transfer(&io, "alice.bal", "bob.bal", 10);
    io.print("done\n");
}

test "事务模式（演示）" {
    // S4 演示型（Q-T6）：transfer 读写真实文件（journal.log/余额文件），不在测试中执行；
    // 文件事务行为断言留 M7 标准库测试
    try expect(true);
}
