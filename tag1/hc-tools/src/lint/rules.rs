//! lint 规则注册表：内置规则定义与查找

use super::models::LintRule;

const RULES: &[LintRule] = &[
    LintRule {
        code: "L001",
        name: "unused_var",
        has_fix: false,
        desc: "未使用变量",
    },
    LintRule {
        code: "L001",
        name: "unused_import",
        has_fix: false,
        desc: "未使用导入",
    },
    LintRule {
        code: "L003",
        name: "simplifiable_construct",
        has_fix: true,
        desc: "可简化构造",
    },
    LintRule {
        code: "L004",
        name: "upper_case_abbr",
        has_fix: true,
        desc: "缩写应全大写",
    },
    LintRule {
        code: "L005",
        name: "simplifiable_if_else",
        has_fix: true,
        desc: "可简化 if-else",
    },
    LintRule {
        code: "L006",
        name: "redundant_eq_false",
        has_fix: true,
        desc: "多余的 == false",
    },
];

pub fn all_rules() -> &'static [LintRule] {
    RULES
}

pub fn find_rule(name: &str) -> Option<&'static LintRule> {
    RULES.iter().find(|r| r.name == name)
}
