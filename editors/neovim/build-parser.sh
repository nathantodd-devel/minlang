#!/bin/sh
# Builds the tree-sitter parser Neovim loads from parser/minlang.so.
set -e
here=$(cd "$(dirname "$0")" && pwd)
grammar="$here/../tree-sitter-minlang/src"
mkdir -p "$here/parser"
${CC:-cc} -shared -fPIC -O2 -I "$grammar" "$grammar/parser.c" -o "$here/parser/minlang.so"
echo "built $here/parser/minlang.so"
