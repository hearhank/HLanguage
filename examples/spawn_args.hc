// 带参 spawn：C 后端验证（双后端一致性）
// h run examples/spawn_args.hc 与 h build examples/spawn_args.hc --exec 输出必须完全一致
// 参数经打包结构体传入 Fiber 入口（h_sp_ctx_f + h_task_f）

fun w(id: u64) {
    print("执行体", id.to_str())
}

fun f(a: u64, b: u64) {
    print("求和:", (a + b).to_str())
}

spawn w(1)
spawn f(2, 3)
spawn w(4)
