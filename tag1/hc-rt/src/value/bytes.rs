use super::models::Value;

impl Value {
    /// 从 `&[u8]` 或 `String` 提取字节（用于 IO/FS 等需要字节数据的函数）
    pub fn extract_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Value::String(s) => Some(s.as_slice().to_vec()),
            // H4 切片视图：物化 data[start..start+len] 字节（interp 链路派生切片
            // 传入 host fs/字符串内建的合法路径——K5 S8 修复）
            Value::Slice { data, start, len } => {
                let cells = data.borrow();
                let mut out = Vec::with_capacity(*len);
                let mut i = *start;
                while i < start + len && i < cells.len() {
                    let v = cells[i].borrow();
                    match &*v {
                        Value::Int(b) => out.push(*b as u8),
                        Value::String(s) => {
                            out.extend_from_slice(s.as_slice());
                        }
                        _ => return None,
                    }
                    i += 1;
                }
                Some(out)
            }
            _ => None,
        }
    }
}
