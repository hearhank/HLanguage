//! LZ77 压缩（G4 io.archive 通用压缩算法）——纯函数共享层（ADR-0004 语义唯一源）
//!
//! 替代原有 RLE 压缩，提供更通用的 LZ77 滑动窗口压缩。
//! 格式：
//!   `0x00` = 字面跑（次字节 = 长度-1，后随该长原始字节，实际 1..=256 字节）
//!   `0x01` = 反向引用（次字节 = 长度-3，再 2 字节 = 距离 u16 LE，实际 3..=258 字节，距离 1..=65535）
//!   `0x02` = 重复跑（次字节 = 计数-1，再一字节 = 值，实际 1..=256 次重复）
//! round-trip 对任意输入保真。

/// 滑动窗口大小（4KB）
const WINDOW_SIZE: usize = 4096;
/// 最小匹配长度
const MIN_MATCH: usize = 3;
/// 最大匹配长度（受 u8 长度域限制：0..=255 → 实际 3..=258）
const MAX_MATCH: usize = 258;

/// LZ77 压缩：对 `data` 进行滑动窗口压缩，返回压缩后的字节序列。
/// 对任意输入保真（round-trip 可靠）。
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let len = data.len();

    while i < len {
        // 第一步：检查当前字节是否形成重复跑（≥3 相同字节）
        // 若有，用 token 0x02 高效编码（3 字节，而非 0x01 的 4 字节）
        let b = data[i];
        let mut run_end = i + 1;
        while run_end < len && data[run_end] == b && run_end - i < 256 {
            run_end += 1;
        }
        let run_len = run_end - i;

        if run_len >= MIN_MATCH {
            // 重复跑 → token 0x02（RLE 式）
            out.push(0x02);
            out.push((run_len - 1) as u8);
            out.push(b);
            i = run_end;
            continue;
        }

        // 第二步：在滑动窗口中查找最长匹配
        let window_start = i.saturating_sub(WINDOW_SIZE);
        let max_lookahead = (len - i).min(MAX_MATCH);

        let (match_len, match_dist) = if max_lookahead >= MIN_MATCH {
            find_longest_match(data, window_start, i, i, max_lookahead)
        } else {
            (0, 0)
        };

        if match_len >= MIN_MATCH {
            // 输出反向引用：0x01 + (length-3) + distance(u16 LE)
            out.push(0x01);
            out.push((match_len - 3) as u8);
            out.push((match_dist & 0xFF) as u8);
            out.push((match_dist >> 8) as u8);
            i += match_len;
        } else {
            // 聚合字面跑：至多 256 字节
            let lit_start = i;
            let mut lit_end = i;
            while lit_end < len && lit_end - lit_start < 256 {
                let lookahead = (len - lit_end).min(MAX_MATCH);
                if lookahead >= MIN_MATCH {
                    // 检查重复跑截断点
                    let nb = data[lit_end];
                    let mut nr = lit_end + 1;
                    while nr < len && data[nr] == nb && nr - lit_end < 256 {
                        nr += 1;
                    }
                    if nr - lit_end >= MIN_MATCH {
                        break;
                    }
                    // 检查 LZ77 匹配截断点
                    let (ml, _) = find_longest_match(
                        data,
                        lit_end.saturating_sub(WINDOW_SIZE),
                        lit_end,
                        lit_end,
                        lookahead,
                    );
                    if ml >= MIN_MATCH {
                        break;
                    }
                }
                lit_end += 1;
            }
            let lit_count = (lit_end - lit_start).max(1);
            out.push(0x00);
            out.push((lit_count - 1) as u8);
            out.extend_from_slice(&data[lit_start..lit_start + lit_count]);
            i = lit_start + lit_count;
        }
    }

    out
}

/// LZ77 解压：对 LZ77 压缩数据进行解压。
/// 非法 token / 越界 → Err。
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < data.len() {
        match data[i] {
            0x00 => {
                // 字面跑
                if i + 1 >= data.len() {
                    return Err(());
                }
                let count = data[i + 1] as usize + 1;
                i += 2;
                if i + count > data.len() {
                    return Err(());
                }
                out.extend_from_slice(&data[i..i + count]);
                i += count;
            }
            0x01 => {
                // 反向引用
                if i + 3 >= data.len() {
                    return Err(());
                }
                let length = data[i + 1] as usize + 3;
                let dist_low = data[i + 2] as usize;
                let dist_high = data[i + 3] as usize;
                let distance = dist_low | (dist_high << 8);
                i += 4;

                if distance == 0 || distance > out.len() {
                    return Err(());
                }
                let start = out.len() - distance;
                // 逐字节复制（处理重叠：如 distance=1, length=10 → 重复同一字节）
                for j in 0..length {
                    out.push(out[start + j]);
                }
            }
            0x02 => {
                // 重复跑（RLE 式）
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

/// 在 `data[search_start..search_end]` 中查找与 `data[pos..]` 的最长匹配。
/// 返回 (匹配长度, 匹配距离)。
fn find_longest_match(
    data: &[u8],
    search_start: usize,
    search_end: usize,
    pos: usize,
    max_lookahead: usize,
) -> (usize, usize) {
    let mut best_len = 0usize;
    let mut best_dist = 0usize;

    // 简单扫描窗口（窗口 ≤ 4KB，O(n*m) 可接受；后续可优化为 hash chain）
    let window_len = search_end - search_start;
    let scan_start = if window_len > 256 {
        search_end - 256
    } else {
        search_start
    };

    let mut j = scan_start;
    while j < search_end {
        // 快速失败：首字节不匹配跳过
        if data[j] != data[pos] {
            j += 1;
            continue;
        }

        let mut ml = 1usize;
        while ml < max_lookahead
            && j + ml < search_end
            && pos + ml < data.len()
            && data[j + ml] == data[pos + ml]
        {
            ml += 1;
        }

        if ml > best_len {
            best_len = ml;
            best_dist = pos - j;
            if best_len == max_lookahead {
                break; // 已达最大可能匹配长度
            }
        }
        j += 1;
    }

    (best_len, best_dist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_empty() {
        let data = b"";
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_small() {
        let data = b"hello";
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_repeated() {
        let data = b"aaaaabbbbbcccccdddddeeeee";
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_short_runs() {
        // "aaabbbccccc" — 11 字节，RLE 可压缩至 < 11
        let data = b"aaabbbccccc";
        let compressed = compress(data);
        assert!(
            compressed.len() < data.len(),
            "short runs should compress with RLE token"
        );
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_binary() {
        let data = &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_token_bytes() {
        // 包含 token 字节 0x00/0x01/0x02 的字面数据
        let data = &[
            0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02,
        ];
        let compressed = compress(data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_long_repetition() {
        // 超过 256 字节的重复段
        let data = vec![0xABu8; 500];
        let compressed = compress(&data);
        assert!(
            compressed.len() < data.len(),
            "long repetition should compress well"
        );
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_mixed() {
        // 混合数据：重复段 + 随机段
        let mut data = Vec::new();
        data.extend_from_slice(b"hello world hello world hello world");
        data.extend_from_slice(b"this is some unique text");
        data.extend_from_slice(&[0x42; 100]);
        let compressed = compress(&data);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn decompress_invalid_short() {
        assert!(decompress(b"\x00").is_err());
        assert!(decompress(b"\x01").is_err());
        assert!(decompress(b"\x01\x00").is_err());
        assert!(decompress(b"\x01\x00\x01").is_err());
        assert!(decompress(b"\x02").is_err());
        assert!(decompress(b"\xff").is_err());
    }

    #[test]
    fn decompress_invalid_distance() {
        // 距离超过已输出长度
        let compressed = b"\x01\x00\x01\x00"; // distance=1, 但 out 为空
        assert!(decompress(compressed).is_err());
    }

    #[test]
    fn compress_improves_on_rle() {
        // 对"ababab"这种 RLE 不佳的模式，LZ77 应压缩更好
        let data = b"abababababababababababababababab"; // 32 bytes
        let compressed = compress(data);
        // RLE 对此类数据几乎无压缩效果（每个字节不同）
        // LZ77 应能识别 "ab" 重复模式
        assert!(
            compressed.len() < data.len(),
            "LZ77 should compress alternating pattern"
        );
    }

    #[test]
    fn decompress_invalid_token() {
        assert!(decompress(b"\x02\x00").is_err()); // 0x02 缺少 value 字节
    }
}
