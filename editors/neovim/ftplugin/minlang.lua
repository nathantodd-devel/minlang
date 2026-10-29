-- minlang buffers: comments, tree-sitter highlighting and the language server.

vim.bo.commentstring = '// %s'
vim.bo.shiftwidth = 4
vim.bo.expandtab = true

-- Needs the parser built by build-parser.sh; without it there's no highlighting
pcall(vim.treesitter.start)

if vim.fn.executable('minlang') == 1 then
  vim.lsp.start({
    name = 'minlang',
    cmd = { 'minlang', 'lsp' },
    root_dir = vim.fs.root(0, { '.git' }) or vim.fn.getcwd(),
  })
end
