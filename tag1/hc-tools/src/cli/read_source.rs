//! 源文件读取辅助

use std::path::Path;
use std::process::ExitCode;

pub(crate) fn read_source(path: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        ExitCode::FAILURE
    })
}
