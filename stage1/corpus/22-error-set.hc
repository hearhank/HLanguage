// 22-error-set.hc — 错误集分析测试
fn test_error_ok() !void {
    return error.NotFound;
}
fn test_error_fail() void {
    return error.NotFound;
}