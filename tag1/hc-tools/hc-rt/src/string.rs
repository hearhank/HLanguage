//! String 值类型：堆分配的字节数组（值语义，克隆即深拷贝）
//!
//! 定义：结构体：StringData
//!
//! 与 `hc::ir::string::StringDataIr` 结构相同。
//! 由 `Vec<u8>` 支持，无固定长度限制。

/// String 值类型：堆分配的字节数组（值语义，克隆即深拷贝）
///
/// 与 `hc::ir::string::StringDataIr` 结构相同。
#[derive(Debug, Clone)]
pub struct StringData {
    buf: Vec<u8>,
}

impl Default for StringData {
    fn default() -> Self {
        Self { buf: Vec::new() }
    }
}

impl StringData {
    /// 创建空字符串（零初始化）
    pub fn new() -> Self {
        Self::default()
    }

    /// 从字节切片复制数据创建 String
    pub fn from_slice(slice: &[u8]) -> Self {
        Self {
            buf: Vec::from(slice),
        }
    }

    /// 从字节向量创建 String（获取所有权）
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { buf: bytes }
    }

    /// 返回内部字节的借用视图
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// 字节长度
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否为空字符串
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
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
        let s2 = s1.clone();
        assert_eq!(s2.as_slice(), b"hello world");
        assert_eq!(s1.as_slice(), b"hello world");
        assert_eq!(s1.len(), s2.len());
    }

    #[test]
    fn test_from_bytes() {
        let s = StringData::from_bytes(vec![104, 101, 108, 108, 111]);
        assert_eq!(s.len(), 5);
        assert_eq!(s.as_slice(), b"hello");
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
