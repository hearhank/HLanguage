use std::cell::RefCell;
use std::rc::Rc;

use super::value::Value;

/// 闭包数据（运行时表示；AST 部分由解释器填充）
#[derive(Debug, Clone)]
pub struct ClosureData {
    pub params: Vec<String>,
    pub body: hc::ast::Block,
    pub is_mut: bool,
    pub is_move: bool,
    pub env: Vec<std::collections::HashMap<String, Rc<RefCell<Value>>>>,
}

unsafe impl Send for ClosureData {}
