use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

use super::value::Value;

/// IoC 容器上下文状态（ADR-0026：AppContext / 模块 Context，背靠 Arena）
#[derive(Debug, Clone)]
pub struct ContextState {
    /// 注册表：(type_name, name) → 存储的值
    pub registry: HashMap<String, Value>,
    /// 工厂函数注册表：(type_name, name) → 闭包
    pub factories: HashMap<String, Value>,
    /// 父 context（用于层级委托）
    pub parent: Option<Weak<RefCell<ContextState>>>,
    /// 可用标志
    pub live: bool,
}

impl ContextState {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
            factories: HashMap::new(),
            parent: None,
            live: true,
        }
    }

    /// 注册单例实例（深拷贝到 registry）
    pub fn register(&mut self, type_name: &str, value: Value) {
        self.registry.insert(type_name.to_string(), value);
    }

    /// 命名注册
    pub fn register_named(&mut self, type_name: &str, name: &str, value: Value) {
        let key = format!("{}:{}", type_name, name);
        self.registry.insert(key, value);
    }

    /// 获取注册的实例（查找自身，未找到时委托父 context）
    pub fn get(&self, type_name: &str) -> Option<Value> {
        if let Some(v) = self.registry.get(type_name) {
            return Some(v.clone());
        }
        // 委托父 context
        if let Some(ref parent) = self.parent {
            if let Some(p) = parent.upgrade() {
                return p.borrow().get(type_name);
            }
        }
        None
    }

    /// 按名获取
    pub fn get_named(&self, type_name: &str, name: &str) -> Option<Value> {
        let key = format!("{}:{}", type_name, name);
        if let Some(v) = self.registry.get(&key) {
            return Some(v.clone());
        }
        if let Some(ref parent) = self.parent {
            if let Some(p) = parent.upgrade() {
                return p.borrow().get_named(type_name, name);
            }
        }
        None
    }

    /// 注册工厂
    pub fn register_factory(&mut self, name: &str, factory: Value) {
        self.factories.insert(name.to_string(), factory);
    }

    /// 获取工厂
    pub fn get_factory(&self, name: &str) -> Option<Value> {
        if let Some(f) = self.factories.get(name) {
            return Some(f.clone());
        }
        if let Some(ref parent) = self.parent {
            if let Some(p) = parent.upgrade() {
                return p.borrow().get_factory(name);
            }
        }
        None
    }

    /// 清理全部注册
    pub fn deinit(&mut self) {
        self.registry.clear();
        self.factories.clear();
        self.live = false;
    }
}
