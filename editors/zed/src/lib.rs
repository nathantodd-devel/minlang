//! Zed extension for minlang: starts `minlang lsp` for `.pk` files. The
//! grammar and highlighting come from `languages/minlang`.

use zed_extension_api::{self as zed, LanguageServerId, Result};

struct Minlang;

impl zed::Extension for Minlang {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let command = worktree.which("minlang").ok_or_else(|| {
            "`minlang` was not found on the PATH. Install it by running \
             `cargo install --path .` in the minlang repository."
                .to_owned()
        })?;

        Ok(zed::Command {
            command,
            args: vec!["lsp".to_owned()],
            env: Vec::new(),
        })
    }
}

zed::register_extension!(Minlang);
