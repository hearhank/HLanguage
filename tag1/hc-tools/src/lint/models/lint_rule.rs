#[derive(Clone, Copy, Debug)]
pub struct LintRule {
    pub code: &'static str,
    pub name: &'static str,
    pub has_fix: bool,
    pub desc: &'static str,
}
