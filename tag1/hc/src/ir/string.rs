//! String 值类型（IR 侧）：栈上内联缓冲的字节数组（值语义，复制即 memcpy）
//!
//! 与 `hc_rt::string::StringData` 结构相同。
//! 无堆分配，不需要 `deinit()`，作用域退出自动销毁。
//! 字面量创建时编译期检查长度不超过 STRING_BUF_SIZE，超长编译错误。

pub const STRING_BUF_SIZE: usize = 64;

/// String 值类型：栈上内联缓冲的字节数组（值语义，复制即 memcpy）
///
/// 与 `hc_rt::string::StringData` 结构相同。
/// 无堆分配，不需要 `deinit()`，作用域退出自动销毁。
/// 字面量创建时编译期检查长度不超过 STRING_BUF_SIZE，超长编译错误。
#[derive(Debug, Clone, Copy)]
pub struct StringDataIr {
    buf: [u8; STRING_BUF_SIZE],
    len: usize,
}

impl StringDataIr {
    /// 创建空字符串
    pub fn new() -> Self {
        Self {
            buf: [0u8; STRING_BUF_SIZE],
            len: 0,
        }
    }

    /// 从字节切片复制数据创建 String（超出 STRING_BUF_SIZE 的字节 panic）
    pub fn from_slice(slice: &[u8]) -> Self {
        assert!(
            slice.len() <= STRING_BUF_SIZE,
            "String literal exceeds {} bytes",
            STRING_BUF_SIZE
        );
        let mut buf = [0u8; STRING_BUF_SIZE];
        buf[..slice.len()].copy_from_slice(slice);
        Self {
            buf,
            len: slice.len(),
        }
    }

    /// 返回内部字节的借用视图
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// 字节长度
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空字符串
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for StringDataIr {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty() {
        let s = StringDataIr::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.as_slice(), b"");
    }

    #[test]
    fn from_slice_hello() {
        let s = StringDataIr::from_slice(b"hello");
        assert!(!s.is_empty());
        assert_eq!(s.len(), 5);
        assert_eq!(s.as_slice(), b"hello");
    }

    #[test]
    fn from_slice_empty() {
        let s = StringDataIr::from_slice(b"");
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.as_slice(), b"");
    }

    #[test]
    fn clone_creates_independent_copy() {
        let a = StringDataIr::from_slice(b"clone me");
        let mut b = a.clone();
        assert_eq!(a, b);
        // 修改 b 不应影响 a
        b = StringDataIr::from_slice(b"modified");
        assert_ne!(a, b);
    }

    #[test]
    fn copy_semantics() {
        let a = StringDataIr::from_slice(b"copy");
        let b = a; // copy via move
        assert_eq!(a, b); // 值语义：a 依然可用
    }

    #[test]
    fn default_is_empty() {
        let s: StringDataIr = Default::default();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn display_works() {
        let s = StringDataIr::from_slice(b"hello");
        let display = format!("{}", s);
        assert_eq!(display, "hello");
    }

    #[test]
    fn display_utf8_lossy() {
        let s = StringDataIr::from_slice(b"\xff\xfe");
        let display = format!("{}", s);
        // lossy: 非法 UTF-8 被替换为
        assert!(display.contains('\u{FFFD}'));
    }

    #[test]
    fn eq_same_content() {
        let a = StringDataIr::from_slice(b"same");
        let b = StringDataIr::from_slice(b"same");
        assert_eq!(a, b);
    }

    #[test]
    fn eq_different_content() {
        let a = StringDataIr::from_slice(b"abc");
        let b = StringDataIr::from_slice(b"xyz");
        assert_ne!(a, b);
    }
}
