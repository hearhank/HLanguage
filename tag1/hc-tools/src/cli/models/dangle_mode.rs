/// C2（ADR-0016）：Debug 悬垂标记切换模式（编译单元级，`--dangle=on|off|auto`）。
/// `auto` = Debug 开 / Release 关（默认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DangleMode {
    On,
    Off,
    Auto,
}

impl DangleMode {
    /// 返回当前模式是否应启用悬垂检查（Auto 按 Debug 模式处理）。
    pub(crate) fn is_on(self) -> bool {
        match self {
            DangleMode::On => true,
            DangleMode::Off => false,
            DangleMode::Auto => true, // tag1 默认 Debug 开
        }
    }
}
