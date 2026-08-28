/// `hc test` 运行模式：解释器（默认）或原生编译（Q-T5 交叉验证）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestMode {
    Interpret,
    Compile,
}
