use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct AllocBlock {
    pub data: Rc<RefCell<Vec<u8>>>,
    pub offset: usize,
    pub len: usize,
}
