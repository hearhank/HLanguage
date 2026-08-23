//! 特性注册表（Q24 字典式查找）：将特性名称映射到特性的元数据与处理函数。
//!
//! 设计目标：
//! - 系统特性（pad / module / align / test）由编译器默认注册
//! - 用户特性（扩展）可通过注册表 API 注册，目前暂不做
//! - 每个特性可以附带结构化数据（类型或函数），形成一个可扩展的整体
//!
//! 未来方向（Phase 3）：
//! - `IAttribute` 接口用于标记 struct 作为特性类型
//! - 插件式特性处理器：注册表查找 → 分派到对应处理函数

use std::collections::HashMap;

use crate::ast::Trait;
use crate::diag::Diagnostic;
use crate::parser::Parser;

/// 特性参数定义
#[derive(Debug, Clone)]
pub enum TraitParam {
    /// 位置参数：`align(8)` → `TraitParam::Positional { ty: "u32" }`
    Positional { ty: &'static str },
    /// 命名参数：`test(timeout=5)` → `TraitParam::Named { name: "timeout", ty: "u64", optional: true }`
    Named {
        name: &'static str,
        ty: &'static str,
        optional: bool,
    },
}

/// 特性元数据：描述一个特性的名称、参数和构建方式
#[derive(Debug, Clone)]
pub struct TraitInfo {
    /// 特性名称（小写）
    pub name: &'static str,
    /// 参数列表
    pub params: &'static [TraitParam],
    /// 简要说明
    pub description: &'static str,
}

/// 特性处理器：从解析器解析特性参数并构造 `Trait` 值
pub type TraitHandlerFn = fn(&mut Parser) -> Result<Trait, Diagnostic>;

/// 特性注册表：字典式查找特性名称 → 元数据 + 处理器
///
/// 初始化时自动注册所有系统特性。
/// 可通过 `register` 方法扩展用户特性（暂不支持）。
#[derive(Debug)]
pub struct TraitRegistry {
    /// 名称 → 元数据
    traits: HashMap<&'static str, TraitInfo>,
    /// 名称 → 处理器函数
    handlers: HashMap<&'static str, TraitHandlerFn>,
}

impl TraitRegistry {
    /// 创建新注册表，自动注册所有系统特性
    pub fn new() -> Self {
        let mut reg = Self {
            traits: HashMap::new(),
            handlers: HashMap::new(),
        };
        reg.register_system_traits();
        reg
    }

    /// 注册系统特性
    fn register_system_traits(&mut self) {
        // [pad]：紧凑布局，字段间无填充
        self.register(TraitInfo {
            name: "pad",
            params: &[],
            description: "紧凑布局，字段间无填充，alignOf = 1",
        });
        // [module]：命名空间隔离
        self.register(TraitInfo {
            name: "module",
            params: &[],
            description: "命名空间模块隔离，内容不参与同包共享命名空间",
        });
        // [align(N)]：类型级对齐
        self.register(TraitInfo {
            name: "align",
            params: &[TraitParam::Positional { ty: "u32" }],
            description: "类型级对齐，尾部圆整到 N 字节（1/2/4/8）",
        });
        // [test] / [test("name")] / [test(async)] / [test(thread)] / [test(timeout=N)]
        self.register(TraitInfo {
            name: "test",
            params: &[
                TraitParam::Named {
                    name: "name",
                    ty: "?String",
                    optional: true,
                },
                TraitParam::Named {
                    name: "mode",
                    ty: "TestMode",
                    optional: true,
                },
                TraitParam::Named {
                    name: "timeout",
                    ty: "?u64",
                    optional: true,
                },
            ],
            description: "测试函数标记，支持名称/模式(async/thread)/超时(timeout=N)",
        });
    }

    /// 注册一个特性
    pub fn register(&mut self, info: TraitInfo) {
        self.traits.insert(info.name, info);
    }

    /// 注册一个特性处理器
    pub fn register_handler(&mut self, name: &'static str, handler: TraitHandlerFn) {
        self.handlers.insert(name, handler);
    }

    /// 按名称查找特性
    pub fn lookup(&self, name: &str) -> Option<&TraitInfo> {
        self.traits.get(name)
    }

    /// 按名称查找特性处理器
    pub fn lookup_handler(&self, name: &str) -> Option<&TraitHandlerFn> {
        self.handlers.get(name)
    }

    /// 检查特性名称是否已注册
    pub fn is_known(&self, name: &str) -> bool {
        self.traits.contains_key(name)
    }

    /// 获取所有已注册的特性名称
    pub fn known_names(&self) -> Vec<&str> {
        self.traits.keys().copied().collect()
    }
}

impl Default for TraitRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_traits_registered() {
        let reg = TraitRegistry::new();
        assert!(reg.is_known("pad"));
        assert!(reg.is_known("module"));
        assert!(reg.is_known("align"));
        assert!(reg.is_known("test"));
        assert!(!reg.is_known("unknown_trait"));
    }

    #[test]
    fn test_lookup_returns_info() {
        let reg = TraitRegistry::new();
        let align = reg.lookup("align").expect("align should be registered");
        assert_eq!(align.name, "align");
        assert_eq!(align.params.len(), 1);
        let pad = reg.lookup("pad").expect("pad should be registered");
        assert_eq!(pad.params.len(), 0);
    }

    #[test]
    fn test_handler_registration_and_lookup() {
        let mut reg = TraitRegistry::new();
        // 注册一个测试处理器
        fn test_handler(
            _p: &mut crate::parser::Parser,
        ) -> Result<crate::ast::Trait, crate::diag::Diagnostic> {
            Ok(crate::ast::Trait::Pad)
        }
        reg.register_handler("custom", test_handler);
        assert!(reg.lookup_handler("custom").is_some());
        assert!(reg.lookup_handler("unknown").is_none());
        assert!(reg.lookup_handler("pad").is_none()); // pad 有 info 但无 handler
    }
}
