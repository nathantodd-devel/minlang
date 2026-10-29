// Tokenizes minlang code with the TextMate grammar using the same engine as
// VS Code, and checks the scopes given to sample tokens.

const fs = require('fs');
const path = require('path');
const assert = require('assert');
const vsctm = require('vscode-textmate');
const oniguruma = require('vscode-oniguruma');

const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm')).buffer;
const onigLib = oniguruma.loadWASM(wasm).then(() => ({
  createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
  createOnigString: (text) => new oniguruma.OnigString(text),
}));

const registry = new vsctm.Registry({
  onigLib,
  loadGrammar: async () =>
    vsctm.parseRawGrammar(
      fs.readFileSync(path.join(__dirname, '../syntaxes/minlang.tmLanguage.json'), 'utf8'),
      'minlang.tmLanguage.json',
    ),
});

/** Every token of each line of `source` as [text, most specific scope]. */
function tokenize(grammar, source) {
  const lines = [];
  let state = vsctm.INITIAL;
  for (const line of source.split('\n')) {
    const result = grammar.tokenizeLine(line, state);
    lines.push(
      result.tokens.map((token) => [
        line.slice(token.startIndex, token.endIndex),
        token.scopes[token.scopes.length - 1],
      ]),
    );
    state = result.ruleStack;
  }
  return lines;
}

function scopeOf(lines, line, text) {
  const token = lines[line].find(([tokenText]) => tokenText.trim() === text);
  assert.ok(token, `no token ${JSON.stringify(text)} on line ${line}`);
  return token[1];
}

(async () => {
  const grammar = await registry.loadGrammar('source.minlang');

  const tokens = tokenize(
    grammar,
    [
      'import std::printf; // say hello',
      'struct Point { x: f32 }',
      'fn area(shape: Shape) -> f32 {',
      '    let total: u32 = 0x40u8 as u32 + 2.5f32;',
      '    switch shape { case Shape::Circle(r) { return r; } }',
      '    show("%d\\n", p.x);',
      '}',
    ].join('\n'),
  );

  // [line, token, scope]
  const expected = [
    [0, 'import', 'keyword.control.import.minlang'],
    [0, 'std', 'entity.name.namespace.minlang'],
    [0, '// say hello', 'comment.line.double-slash.minlang'],
    [1, 'struct', 'storage.type.minlang'],
    [1, 'Point', 'entity.name.type.minlang'],
    [1, 'f32', 'support.type.primitive.minlang'],
    [2, 'fn', 'storage.type.function.minlang'],
    [2, 'area', 'entity.name.function.minlang'],
    [2, 'Shape', 'entity.name.type.minlang'],
    [2, '->', 'keyword.operator.minlang'],
    [3, 'let', 'storage.type.minlang'],
    [3, 'total', 'variable.other.minlang'],
    [3, '0x40u8', 'constant.numeric.integer.minlang'],
    [3, 'as', 'keyword.operator.cast.minlang'],
    [3, '2.5f32', 'constant.numeric.float.minlang'],
    [4, 'switch', 'keyword.control.minlang'],
    [4, 'Circle', 'variable.other.enummember.minlang'],
    [4, 'return', 'keyword.control.minlang'],
    [5, 'show', 'entity.name.function.call.minlang'],
    [5, '\\n', 'constant.character.escape.minlang'],
    [5, 'x', 'variable.other.property.minlang'],
  ];
  for (const [line, text, scope] of expected) {
    assert.strictEqual(scopeOf(tokens, line, text), scope, `${text} on line ${line}`);
  }

  // Every example tokenizes without the grammar choking
  const examples = path.join(__dirname, '../../../examples');
  for (const file of fs.readdirSync(examples).filter((name) => name.endsWith('.pk'))) {
    tokenize(grammar, fs.readFileSync(path.join(examples, file), 'utf8'));
  }

  console.log(`ok: ${expected.length} scopes checked`);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
