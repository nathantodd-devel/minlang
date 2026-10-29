; Highlights for Neovim and Zed. Patterns don't overlap, so their order
; doesn't matter (editors disagree on which overlapping pattern wins).
; editors/helix/highlights.scm is the same with Helix's capture names.

(comment) @comment

; Literals

(string_literal) @string
(char_literal) @character
(escape_sequence) @string.escape
(integer_literal) @number
(float_literal) @number.float
(boolean_literal) @boolean
(at) @variable.builtin

; Types

(primitive_type) @type.builtin
(type_identifier) @type
(scoped_type_identifier path: (identifier) @module)
(array_type "?" @type.builtin)

; Functions

(function_item name: (identifier) @function)
(call_expression
  function: (identifier) @function.call
  (#match? @function.call "^[a-z_]"))
(call_expression
  function: (scoped_identifier
    name: (identifier) @function.call)
  (#match? @function.call "^[a-z_]"))

; Modules and enum variants: `std::printf`, `Shape::Circle`

(scoped_identifier
  path: (identifier) @module
  (#match? @module "^[a-z_]"))
(scoped_identifier
  path: (identifier) @type
  (#match? @type "^[A-Z]"))
(scoped_identifier
  name: (identifier) @constructor
  (#match? @constructor "^[A-Z]"))
(enum_variant name: (identifier) @constructor)
(variant_pattern variant: (identifier) @constructor)
(struct_variant_pattern variant: (identifier) @constructor)
(import_declaration path: (identifier) @module)

; Variables, parameters and fields

(parameter name: (identifier) @variable.parameter)
(let_declaration name: (identifier) @variable)
(binding) @variable
(for_expression binding: (identifier) @variable)
(field_declaration name: (field_identifier) @property)
(field_initializer field: (field_identifier) @property)
(field_expression field: (field_identifier) @property)
(field_pattern field: (field_identifier) @property)

; Keywords

["fn" "struct" "enum"] @keyword.type
["let" "const"] @keyword
"pub" @keyword.modifier
"import" @keyword.import
["if" "else" "switch" "case" "default"] @keyword.conditional
["while" "loop" "do" "for" "in" (break_expression) (continue_expression)] @keyword.repeat
["return" "yield"] @keyword.return
["as" "iter"] @keyword.operator

; Operators and punctuation

[
  "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "+" "-" "*" "/" "%" "&" "|" "^" "<<" ">>" "!" "&&" "||"
  "==" "!=" "<" ">" "<=" ">=" "++" "--" "->"
] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "::" "."] @punctuation.delimiter
