//! ANSI 颜色输出辅助（终端检测 + NO_COLOR）

use std::io::IsTerminal;

/// ANSI 颜色开关：仅当目标流为终端且未设置 NO_COLOR 时启用。
/// 重定向/管道（CI、check-examples.sh 捕获）下自动关闭，保证 grep 解析不受污染。
pub(crate) fn out_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
pub(crate) fn err_color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}
/// 给文本涂 ANSI 颜色（on=false 时原样返回）。
pub(crate) fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
/// 单行测试结果标记上色：[PASS] 绿 / [FAIL] 红 / [SKIP] 黄，其余原样。
pub(crate) fn color_test_line(line: &str, on: bool) -> String {
    if !on {
        return line.to_string();
    }
    for (tag, code) in [("[PASS]", "32"), ("[FAIL]", "31"), ("[SKIP]", "33")] {
        if let Some(rest) = line.strip_prefix(tag) {
            return format!("{}{rest}", paint(true, code, tag));
        }
    }
    line.to_string()
}
