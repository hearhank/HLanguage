use std::collections::VecDeque;

use super::value::Value;

#[derive(Debug)]
pub struct ChanInner {
    pub queue: VecDeque<Value>,
    pub closed: bool,
}
