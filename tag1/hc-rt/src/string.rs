//! String 值类型：栈上内联缓冲的字节数组（值语义，复制即 memcpy）
//!
//! 定义：结构体：StringData
//!
//! 与 `hc::ir::string::StringDataIr` 结构相同。
//! 无堆分配，不需要 `deinit()`，作用域退出自动销毁。
//! 字面量创建时编译期检查长度不超过 `STRING_BUF_SIZE`，超长编译错误。

pub const STRING_BUF_SIZE: usize = 64;

/// String 值类型：栈上内联缓冲的字节数组（值语义，复制即 memcpy）
///
/// 与 `hc::ir::string::StringDataIr` 结构相同。
/// 无堆分配，不需要 `deinit()`，作用域退出自动销毁。
/// 字面量创建时编译期检查长度不超过 `STRING_BUF_SIZE`，超长编译错误。
#[derive(Debug, Clone, Copy)]
pub struct StringData {
    buf: [u8; STRING_BUF_SIZE],
    len: usize,
}

impl Default for StringData {
    fn default() -> Self {
        Self {
            buf: [0u8; STRING_BUF_SIZE],
            len: 0,
        }
    }
}

impl StringData {
    /// 创建空字符串（零初始化）
    pub fn new() -> Self {
        Self::default()
    }

    /// 从字节切片复制数据创建 String（超出 STRING_BUF_SIZE 的字节 panic）
    pub fn from_slice(slice: &[u8]) -> Self {
        assert!(
            slice.len() <= STRING_BUF_SIZE,
            "String literal exceeds {} bytes",
            STRING_BUF_SIZE
        );
        let mut s = Self::new();
        s.len = slice.len();
        s.buf[..slice.len()].copy_from_slice(slice);
        s
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
        let s = StringData::from_slice(b"hello");
        assert_eq!(s.len(), 5);
        assert_eq!(s.as_slice(), b"hello");
    }

    #[test]
    fn test_from_slice_empty() {
        let s = StringData::from_slice(b"");
        assert!(s.is_empty());
        assert!(s.as_slice().is_empty());
    }

    #[test]
    fn test_clone() {
        let s1 = StringData::from_slice(b"hello world");
        let s2 = s1;
        // Copy 语义：s1 仍然有效
        assert_eq!(s2.as_slice(), b"hello world");
        assert_eq!(s1.as_slice(), b"hello world");
        assert_eq!(s1.len(), s2.len());
    }

    #[test]
    fn test_eq() {
        let s1 = StringData::from_slice(b"hello");
        let s2 = StringData::from_slice(b"hello");
        let s3 = StringData::from_slice(b"world");
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_display() {
        let s = StringData::from_slice("hello 世界".as_bytes());
        assert_eq!(format!("{s}"), "hello 世界");
    }
}
