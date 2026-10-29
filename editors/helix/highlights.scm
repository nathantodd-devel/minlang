; Highlights for Helix: the same patterns as
; editors/tree-sitter-minlang/queries/highlights.scm, with Helix's capture
; names. tests/editors.rs checks the two stay in step.

(comment) @comment

; Literals

(string_literal) @string
(char_literal) @constant.character
(escape_sequence) @constant.character.escape
(integer_literal) @constant.numeric.integer
(float_literal) @constant.numeric.float
(boolean_literal) @constant.builtin.boolean
(at) @variable.builtin

; Types

(primitive_type) @type.builtin
(type_identifier) @type
(scoped_type_identifier path: (identifier) @namespace)
(array_type "?" @type.builtin)

; Functions

(function_item name: (identifier) @function)
(extern_function_item name: (identifier) @function)
(call_expression
  function: (identifier) @function
  (#match? @function "^[a-z_]"))
(call_expression
  function: (scoped_identifier
    name: (identifier) @function)
  (#match? @function "^[a-z_]"))

; Modules and enum variants: `std::printf`, `Shape::Circle`

(scoped_identifier
  path: (identifier) @namespace
  (#match? @namespace "^[a-z_]"))
(scoped_identifier
  path: (identifier) @type
  (#match? @type "^[A-Z]"))
(scoped_identifier
  name: (identifier) @constructor
  (#match? @constructor "^[A-Z]"))
(enum_variant name: (identifier) @constructor)
(variant_pattern variant: (identifier) @constructor)
(struct_variant_pattern variant: (identifier) @constructor)
(namespace_item name: (identifier) @namespace)
(import_declaration path: (identifier) @namespace)

; Variables, parameters and fields

(parameter name: (identifier) @variable.parameter)
(let_declaration name: (identifier) @variable)
(binding) @variable
(for_expression binding: (identifier) @variable)
(field_declaration name: (field_identifier) @variable.other.member)
(field_initializer field: (field_identifier) @variable.other.member)
(field_expression field: (field_identifier) @variable.other.member)
(field_pattern field: (field_identifier) @variable.other.member)

; Keywords

["fn" "struct" "enum" "namespace"] @keyword.storage.type
["let" "const"] @keyword
["pub" "extern"] @keyword.storage.modifier
"import" @keyword.control.import
["if" "else" "switch" "case" "default"] @keyword.control.conditional
["while" "loop" "do" "for" "in" (break_expression) (continue_expression)] @keyword.control.repeat
["return" "yield"] @keyword.control.return
["as" "iter"] @keyword.operator

; Operators and punctuation

[
  "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "+" "-" "*" "/" "%" "&" "|" "^" "<<" ">>" "!" "&&" "||"
  "==" "!=" "<" ">" "<=" ">=" "++" "--" "->"
] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "::" "."] @punctuation.delimiter
