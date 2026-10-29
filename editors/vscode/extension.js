// Starts `minlang lsp` for minlang files. Highlighting comes from the
// TextMate grammar, refined by the server's semantic tokens.

const { workspace, window } = require('vscode');
const { LanguageClient } = require('vscode-languageclient/node');

let client;

function activate() {
  const command = workspace.getConfiguration('minlang').get('server.path') || 'minlang';

  client = new LanguageClient(
    'minlang',
    'minlang',
    { command, args: ['lsp'] },
    { documentSelector: [{ scheme: 'file', language: 'minlang' }] },
  );

  client.start().catch((error) => {
    window.showErrorMessage(
      `Couldn't start the minlang language server (${command} lsp): ${error.message}. ` +
        'Install it with `cargo install --path .` in the minlang repository, ' +
        'or set `minlang.server.path`.',
    );
  });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
