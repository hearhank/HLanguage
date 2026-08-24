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
        let command = worktree.which("hc-lsp").ok_or_else(|| {
            "hc-lsp not found in PATH. Run setup-lsp.bat or add bin/ to PATH.".to_string()
        })?;
        Ok(zed::Command {
            command,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(HLanguageExtension);
