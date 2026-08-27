//! String 值类型（IR 侧）：堆分配的字节数组
//!
//! 与 `hc_rt::string::StringData` 结构相同。

/// String 值类型：堆分配的字节数组
#[derive(Debug, Clone)]
pub struct StringDataIr(Vec<u8>);

impl StringDataIr {
    /// 创建空字符串
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// 从字节切片复制数据创建 String
    pub fn from_slice(slice: &[u8]) -> Self {
        Self(Vec::from(slice))
    }

    /// 从 Vec<u8> 创建 String
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// 返回内部字节的借用视图
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// 字节长度
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 是否为空字符串
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for StringDataIr {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for StringDataIr {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for StringDataIr {}

impl std::fmt::Display for StringDataIr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
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
    fn from_bytes() {
        let s = StringDataIr::from_bytes(b"world".to_vec());
        assert_eq!(s.len(), 5);
        assert_eq!(s.as_slice(), b"world");
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
