//! RLE 压缩（G4 io.archive）——纯函数共享层（ADR-0004 语义唯一源）

/// G4（E3.3 archive）：RLE 压缩——token `0x00` = 字面跑（次字节 = 长度-1，后随该长
/// 原始字节）、`0x01` = 重复跑（次字节 = 计数-1，再一字节 = 值）。长度域 u8
/// （0..=255 → 实际 1..=256）。连续 ≥3 的相同字节 → 重复跑；否则聚合为字面跑
/// （至多 256 字节，遇起始 ≥3 的相同段即断）。round-trip 对任意输入保真。
pub fn encode_rle(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        // 自 i 起的相同字节跑长（≤256）
        let b = data[i];
        let mut j = i + 1;
        while j < data.len() && data[j] == b && j - i < 256 {
            j += 1;
        }
        let run = j - i;
        if run >= 3 {
            out.push(0x01);
            out.push((run - 1) as u8);
            out.push(b);
            i = j;
        } else {
            // 聚合字面跑：至多 256 字节；遇起始 ≥3 的相同段即断
            let start = i;
            let mut k = i;
            while k < data.len() && k - start < 256 {
                let nb = data[k];
                let mut kk = k + 1;
                while kk < data.len() && data[kk] == nb && kk - k < 256 {
                    kk += 1;
                }
                if kk - k >= 3 {
                    break;
                }
                k += 1;
            }
            let lits = (k - start).max(1);
            out.push(0x00);
            out.push((lits - 1) as u8);
            out.extend_from_slice(&data[start..start + lits]);
            i = start + lits;
        }
    }
    out
}

/// G4（E3.3 archive）：RLE 解压（encode_rle 逆）；非法 token/越界 → Err（InvalidFormat）。
pub fn decode_rle(data: &[u8]) -> std::result::Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        match data[i] {
            0x00 => {
                if i + 1 >= data.len() {
                    return Err(());
                }
                let len = data[i + 1] as usize + 1;
                i += 2;
                if i + len > data.len() {
                    return Err(());
                }
                out.extend_from_slice(&data[i..i + len]);
                i += len;
            }
            0x01 => {
                if i + 2 >= data.len() {
                    return Err(());
                }
                let count = data[i + 1] as usize + 1;
                let val = data[i + 2];
                out.extend(std::iter::repeat(val).take(count));
                i += 3;
            }
            _ => return Err(()),
        }
    }
    Ok(out)
}
