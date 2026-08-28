use super::models::Value;

impl Value {
    /// 从 `&[u8]` 或 `String` 提取字节（用于 IO/FS 等需要字节数据的函数）
    pub fn extract_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Value::String(s) => Some(s.as_slice().to_vec()),
            _ => None,
        }
    }
}
