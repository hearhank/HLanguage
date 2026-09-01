use zed_extension_api as zed;

struct HLanguageExtension;

impl zed::Extension for HLanguageExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // 1) PATH 查找（标准方式）
        if let Some(path) = worktree.which("hc-lsp") {
            return Ok(zed::Command {
                command: path,
                args: vec![],
                env: Default::default(),
            });
        }

        // 2) 开发环境兜底：<worktree>/bin/hc-lsp[.exe]（setup-lsp.bat 的部署位置）
        let root = worktree.root_path();
        let candidates = [
            format!("{root}/bin/hc-lsp.exe"),
            format!("{root}\\bin\\hc-lsp.exe"),
            format!("{root}/bin/hc-lsp"),
        ];
        for path in candidates {
            if std::fs::metadata(&path).is_ok() {
                return Ok(zed::Command {
                    command: path,
                    args: vec![],
                    env: Default::default(),
                });
            }
        }

        Err("hc-lsp not found. Run setup-lsp.bat / deploy-lsp.bat, or set lsp.hc-lsp.binary.path in Zed settings.".to_string())
    }
}

zed::register_extension!(HLanguageExtension);
