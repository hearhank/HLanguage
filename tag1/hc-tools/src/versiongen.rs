//! `version.hc` 编译时版本号自增（M4-1）。
//!
//! 每次编译前读取 `version.hc`，将 `build` 字段递增、`time` 字段更新为当前 Unix 时间戳。
//! 格式：
//! ```hc
//! const version = Version{
//!     major = 0,
//!     minor = 1,
//!     patch = 0,
//!     build = 42,
//!     time = 1692800000,
//! };
//! ```

use std::path::Path;

/// 对 `version.hc` 执行编译时版本号自增。
/// 返回 `true` 表示成功处理（文件存在且更新），`false` 表示文件不存在或解析失败。
pub(crate) fn bump_version(project_dir: &Path) -> bool {
    let version_path = project_dir.join("version.hc");
    if !version_path.exists() {
        return false;
    }

    let content = match std::fs::read_to_string(&version_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[warn] 读取 version.hc 失败: {e}");
            return false;
        }
    };

    match update_version(&content) {
        Ok(updated) => {
            if let Err(e) = std::fs::write(&version_path, &updated) {
                eprintln!("[warn] 写入 version.hc 失败: {e}");
                return false;
            }
            true
        }
        Err(e) => {
            eprintln!("[warn] 解析 version.hc 失败: {e}");
            false
        }
    }
}

/// 解析并更新 version.hc 内容：递增 `build`，更新 `time` 为当前 Unix 时间戳。
fn update_version(content: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut found_build = false;
    let mut found_time = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // 匹配 `build = <number>,` 或 `build = <number>`
        if let Some(rest) = trimmed.strip_prefix("build =") {
            let num_part = rest.trim_end_matches(',').trim();
            if let Ok(old) = num_part.parse::<u64>() {
                let new = old + 1;
                let indent = leading_whitespace(line);
                if trimmed.ends_with(',') {
                    result.push_str(&format!("{}build = {},\n", indent, new));
                } else {
                    result.push_str(&format!("{}build = {}\n", indent, new));
                }
                found_build = true;
                continue;
            } else {
                return Err(format!("无法解析 build 数值: `{}`", num_part));
            }
        }

        // 匹配 `time = <number>,` 或 `time = <number>`
        if let Some(rest) = trimmed.strip_prefix("time =") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let indent = leading_whitespace(line);
            if trimmed.ends_with(',') {
                result.push_str(&format!("{}time = {},\n", indent, now));
            } else {
                result.push_str(&format!("{}time = {}\n", indent, now));
            }
            found_time = true;
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    if !found_build {
        return Err("version.hc 中未找到 `build` 字段".into());
    }
    if !found_time {
        return Err("version.hc 中未找到 `time` 字段".into());
    }

    Ok(result)
}

/// 提取行首空白（缩进）
fn leading_whitespace(s: &str) -> &str {
    let len = s.len() - s.trim_start().len();
    &s[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_build() {
        let input = "const version = Version{\n    major = 0,\n    minor = 1,\n    patch = 0,\n    build = 41,\n    time = 1000,\n};\n";
        let result = update_version(input).unwrap();
        assert!(result.contains("build = 42,"));
        assert!(!result.contains("build = 41,"));
        // time should be updated to current timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        assert!(result.contains(&format!("time = {}", now)));
    }

    #[test]
    fn test_no_build_field() {
        let input = "const version = Version{\n    major = 0,\n};\n";
        assert!(update_version(input).is_err());
    }

    #[test]
    fn test_build_without_trailing_comma() {
        let input = "const version = Version{\n    build = 5\n    time = 1000,\n};\n";
        let result = update_version(input).unwrap();
        assert!(result.contains("build = 6"));
    }
}
