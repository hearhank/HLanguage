//! String 值类型：拥有所有权的字节数组（值语义，复制即 deep copy）
//!
//! 定义：结构体：StringData
//!
//! 生命周期由编译器管理（作用域出口自动插入 `deinit()`），
//! 用户不可手动调用。`into_array()` 是唯一"逃逸口"——将内部缓冲区
//! 转移给 `Array(u8)`（后者需显式 `defer` 释放）。

use std::alloc::{alloc, dealloc, Layout};

/// 拥有所有权的字节数组（值语义）
///
/// | 状态 | ptr | len | cap |
/// |------|-----|-----|-----|
/// | 空 | null | 0 | 0 |
/// | 非空 | 指向堆内存 | 有效字节数 | 分配容量 |
/// | deinit 后 | null | 0 | 0 |
///
/// # Safety
/// - `ptr` 指向 `Layout::from_size_align(cap, 1)` 分配的堆内存
/// - `len <= cap`
/// - `deinit()` 是唯一释放内存的途径，由编译器负责调用
#[derive(Debug)]
pub struct StringData {
    ptr: *mut u8,
    len: usize,
    cap: usize,
}

unsafe impl Send for StringData {}
unsafe impl Sync for StringData {}

impl StringData {
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
    /// 调用后自身变空
    pub fn take_ptr(&mut self) -> (*mut u8, usize, usize) {
        let ptr = std::mem::replace(&mut self.ptr, std::ptr::null_mut());
        let len = std::mem::take(&mut self.len);
        let cap = std::mem::take(&mut self.cap);
        (ptr, len, cap)
    }
}

impl Default for StringData {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StringData {
    fn clone(&self) -> Self {
        Self::from_slice(self.as_slice())
    }
}

impl PartialEq for StringData {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for StringData {}

impl std::fmt::Display for StringData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.as_slice()))
    }
}

impl Drop for StringData {
    /// 作用域退出时自动释放堆内存（`deinit()` 已调用则 `ptr == null`，安全跳过）
    fn drop(&mut self) {
        self.deinit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let s = StringData::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.as_slice(), &[]);
    }

    #[test]
    fn test_from_slice() {
        let mut s = StringData::from_slice(b"hello");
        assert_eq!(s.len(), 5);
        assert_eq!(s.as_slice(), b"hello");
        s.deinit();
    }

    #[test]
    fn test_from_slice_empty() {
        let s = StringData::from_slice(b"");
        assert!(s.is_empty());
        // 空切片应返回零分配结构（ptr = null）
        assert!(s.as_slice().is_empty());
    }

    #[test]
    fn test_deinit() {
        let mut s = StringData::from_slice(b"hello");
        assert_eq!(s.len(), 5);
        s.deinit();
        assert!(s.is_empty());
        // double-deinit 不应崩溃
        s.deinit();
    }

    #[test]
    fn test_clone() {
        let mut s1 = StringData::from_slice(b"hello world");
        let mut s2 = s1.clone();
        assert_eq!(s2.as_slice(), b"hello world");
        assert_eq!(s1.as_slice(), b"hello world");
        // 验证 deep copy——修改 s1 后 s2 不受影响
        assert_eq!(s1.len(), s2.len());
        s1.deinit();
        s2.deinit();
    }

    #[test]
    fn test_take_ptr() {
        let mut s = StringData::from_slice(b"test");
        let (ptr, len, cap) = s.take_ptr();
        assert!(!ptr.is_null());
        assert_eq!(len, 4);
        assert_eq!(cap, 4);
        assert!(s.is_empty());
        // 取走后需要手动释放指针
        if !ptr.is_null() {
            let layout = Layout::from_size_align(cap, 1).expect("valid layout");
            unsafe {
                dealloc(ptr as *mut u8, layout);
            }
        }
    }

    #[test]
    fn test_eq() {
        let mut s1 = StringData::from_slice(b"hello");
        let mut s2 = StringData::from_slice(b"hello");
        let mut s3 = StringData::from_slice(b"world");
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
        s1.deinit();
        s2.deinit();
        s3.deinit();
    }

    #[test]
    fn test_display() {
        let mut s = StringData::from_slice("hello 世界".as_bytes());
        assert_eq!(format!("{s}"), "hello 世界");
        s.deinit();
    }
}
