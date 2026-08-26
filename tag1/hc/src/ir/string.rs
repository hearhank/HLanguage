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

    /// 从字节切片复制数据创建 String（超出 STRING_BUF_SIZE 的字节被截断）
    pub fn from_slice(slice: &[u8]) -> Self {
        let len = slice.len().min(STRING_BUF_SIZE);
        let mut buf = [0u8; STRING_BUF_SIZE];
        buf[..len].copy_from_slice(&slice[..len]);
        Self { buf, len }
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

    /// 创建新 String 并追加字节（超出缓冲区的字节被截断）
    pub fn concat(&self, other: &[u8]) -> Self {
        let total_len = self.len + other.len();
        let new_len = total_len.min(STRING_BUF_SIZE);
        let mut buf = [0u8; STRING_BUF_SIZE];
        let self_copy_len = self.len.min(new_len);
        buf[..self_copy_len].copy_from_slice(&self.buf[..self_copy_len]);
        let other_copy_len = (new_len - self_copy_len).min(other.len());
        if other_copy_len > 0 {
            buf[self_copy_len..self_copy_len + other_copy_len]
                .copy_from_slice(&other[..other_copy_len]);
        }
        Self { buf, len: new_len }
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
    fn concat_two_strings() {
        let a = StringDataIr::from_slice(b"hello");
        let b = a.concat(b" world");
        assert_eq!(b.as_slice(), b"hello world");
        assert_eq!(b.len(), 11);
    }

    #[test]
    fn concat_empty() {
        let a = StringDataIr::from_slice(b"hello");
        let b = a.concat(b"");
        assert_eq!(b, a);
    }

    #[test]
    fn concat_with_empty_start() {
        let a = StringDataIr::new();
        let b = a.concat(b"hello");
        assert_eq!(b.as_slice(), b"hello");
    }

    #[test]
    fn concat_truncated() {
        let a = StringDataIr::from_slice(b"hello");
        let long = vec![b'x'; STRING_BUF_SIZE];
        let b = a.concat(&long);
        assert_eq!(b.len(), STRING_BUF_SIZE);
        // 前 5 字节来自 "hello"
        assert_eq!(&b.as_slice()[..5], b"hello");
    }

    #[test]
    fn from_slice_truncated() {
        let long = vec![b'a'; STRING_BUF_SIZE + 10];
        let s = StringDataIr::from_slice(&long);
        assert_eq!(s.len(), STRING_BUF_SIZE);
        assert_eq!(s.as_slice(), &long[..STRING_BUF_SIZE]);
    }

    #[test]
    fn concat_self() {
        let a = StringDataIr::from_slice(b"ab");
        let b = a.concat(b"ab");
        assert_eq!(b.as_slice(), b"abab");
        assert_eq!(b.len(), 4);
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
