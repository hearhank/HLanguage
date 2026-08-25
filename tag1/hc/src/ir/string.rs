//! String 值类型（IR 侧）：拥有所有权的字节数组（值语义，复制即 deep copy）
//!
//! 定义：结构体：StringDataIr
//!
//! 生命周期由编译器管理（作用域出口自动插入 `deinit()`），
//! 用户不可手动调用。`into_array()` 是唯一"逃逸口"。

use std::alloc::{alloc, dealloc, Layout};

/// 拥有所有权的字节数组（IR 侧值语义）
///
/// 与 `hc_rt::string::StringData` 结构相同，但独立于解释器运行时。
/// # Safety
/// - `ptr` 指向 `Layout::from_size_align(cap, 1)` 分配的堆内存
/// - `len <= cap`
/// - `deinit()` 是唯一释放内存的途径，由编译器负责调用
#[derive(Debug)]
pub struct StringDataIr {
    ptr: *mut u8,
    len: usize,
    cap: usize,
}

unsafe impl Send for StringDataIr {}
unsafe impl Sync for StringDataIr {}

impl StringDataIr {
    /// 创建空字符串（零分配）
    pub fn new() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// 从字节切片复制数据创建 String（分配 `slice.len()` 字节）
    pub fn from_slice(slice: &[u8]) -> Self {
        if slice.is_empty() {
            return Self::new();
        }
        let layout = Layout::from_size_align(slice.len(), 1).expect("valid layout");
        let ptr = unsafe { alloc(layout) as *mut u8 };
        if ptr.is_null() {
            panic!("out of memory allocating String");
        }
        unsafe {
            std::ptr::copy_nonoverlapping(slice.as_ptr(), ptr, slice.len());
        }
        Self {
            ptr,
            len: slice.len(),
            cap: slice.len(),
        }
    }

    /// 释放堆内存，重置为空字符串
    pub fn deinit(&mut self) {
        if !self.ptr.is_null() {
            let layout = Layout::from_size_align(self.cap, 1).expect("valid layout");
            unsafe {
                dealloc(self.ptr as *mut u8, layout);
            }
            self.ptr = std::ptr::null_mut();
            self.len = 0;
            self.cap = 0;
        }
    }

    /// 返回内部字节的借用视图
    pub fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Deep copy（分配新内存，复制所有字节）
    pub fn clone(&self) -> Self {
        Self::from_slice(self.as_slice())
    }

    /// 是否为空字符串
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节长度
    pub fn len(&self) -> usize {
        self.len
    }

    /// 取走内部指针，转移所有权（用于 `into_array()`）
    pub fn take_ptr(&mut self) -> (*mut u8, usize, usize) {
        let ptr = std::mem::replace(&mut self.ptr, std::ptr::null_mut());
        let len = std::mem::take(&mut self.len);
        let cap = std::mem::take(&mut self.cap);
        (ptr, len, cap)
    }
}

impl Default for StringDataIr {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StringDataIr {
    fn clone(&self) -> Self {
        Self::from_slice(self.as_slice())
    }
}

impl PartialEq for StringDataIr {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for StringDataIr {}

impl std::fmt::Display for StringDataIr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_slice()))
    }
}
