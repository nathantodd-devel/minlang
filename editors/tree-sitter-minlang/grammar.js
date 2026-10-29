/**
 * Tree-sitter grammar for minlang, used for syntax highlighting in Zed,
 * Neovim and Helix. It follows the compiler's parser (src/parser) closely
 * enough to highlight well, but is more lenient: the compiler is what checks
 * programs.
 */

const PREC = {
  assign: 1,
  range: 2,
  logical_or: 3,
  logical_and: 4,
  bit_or: 5,
  bit_xor: 6,
  bit_and: 7,
  equality: 8,
  comparison: 9,
  shift: 10,
  additive: 11,
  multiplicative: 12,
  cast: 13,
  unary: 14,
  postfix: 15,
};

const PRIMITIVES = [
  'u8', 'u16', 'u32', 'u64', 'usize', 's8', 's16', 's32', 's64', 'f32', 'f64', 'bool',
];

const commaSep = (rule) => optional(seq(rule, repeat(seq(',', rule)), optional(',')));

module.exports = grammar({
  name: 'minlang',

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [
    [$._expression, $.struct_variant_pattern, $._type_identifier],
    [$._expression, $._type_identifier],
    // `case Shape::Circle(r)` is a pattern, not a call
    [$._expression, $.variant_pattern],
    [$._expression, $.struct_variant_pattern],
  ],

  supertypes: ($) => [$._expression, $._type],

  rules: {
    source_file: ($) => repeat($._statement),

    comment: (_) => token(seq('//', /[^\n]*/)),

    // -------------------------------------------------------------------------
    // Statements and declarations
    // -------------------------------------------------------------------------

    _statement: ($) =>
      choice(
        $.import_declaration,
        $.function_item,
        $.struct_item,
        $.enum_item,
        $.expression_statement,
        ';',
      ),

    expression_statement: ($) =>
      choice(
        seq($._expression, ';'),
        seq($.increment_expression, ';'),
        prec(1, $._block_like),
      ),

    import_declaration: ($) =>
      seq('import', field('path', $._import_path), ';'),

    _import_path: ($) =>
      choice(
        $.identifier,
        seq($.identifier, '::', $._import_path),
        seq('{', commaSep(choice($.identifier, $.glob)), '}'),
        $.glob,
      ),

    glob: (_) => '*',

    function_item: ($) =>
      seq(
        'fn',
        field('name', $.identifier),
        field('parameters', $.parameters),
        optional(seq('->', field('return_type', $._type))),
        field('body', $.block),
      ),

    parameters: ($) => seq('(', commaSep($.parameter), ')'),

    parameter: ($) => seq(field('name', $.identifier), ':', field('type', $._type)),

    struct_item: ($) =>
      seq('struct', field('name', $._type_identifier), field('body', $.field_declaration_list)),

    field_declaration_list: ($) => seq('{', commaSep($.field_declaration), '}'),

    field_declaration: ($) =>
      seq(field('name', $._field_identifier), ':', field('type', $._type)),

    enum_item: ($) =>
      seq(
        'enum',
        field('name', $._type_identifier),
        optional(seq(':', field('backing_type', $._type))),
        field('body', $.enum_variant_list),
      ),

    enum_variant_list: ($) => seq('{', commaSep($.enum_variant), '}'),

    enum_variant: ($) =>
      seq(
        field('name', $.identifier),
        optional(
          field(
            'fields',
            choice(seq('(', commaSep($._type), ')'), $.field_declaration_list),
          ),
        ),
        optional(seq('=', field('value', $._expression))),
      ),

    let_declaration: ($) =>
      prec.right(
        seq(
          choice('let', 'const'),
          field('name', $.identifier),
          optional(seq(':', field('type', $._type), optional(field('fill', $.block)))),
          optional(seq('=', field('value', $._expression))),
        ),
      ),

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    _type: ($) =>
      choice(
        $.primitive_type,
        $._type_identifier,
        $.scoped_type_identifier,
        $.pointer_type,
        $.reference_type,
        $.iterator_type,
        $.array_type,
        $.parenthesized_type,
      ),

    parenthesized_type: ($) => seq('(', $._type, ')'),

    primitive_type: (_) => choice(...PRIMITIVES),

    scoped_type_identifier: ($) =>
      seq(field('path', $.identifier), '::', field('name', $._type_identifier)),

    pointer_type: ($) => prec.right(seq('*', $._type)),

    reference_type: ($) => prec.right(seq(choice('&', '&&'), $._type)),

    iterator_type: ($) => prec.right(seq('iter', $._type)),

    array_type: ($) =>
      prec.left(seq(field('element', $._type), '[', field('size', choice($.integer_literal, '?')), ']')),

    // -------------------------------------------------------------------------
    // Expressions
    // -------------------------------------------------------------------------

    _expression: ($) =>
      choice(
        $.identifier,
        $.scoped_identifier,
        $.at,
        $.integer_literal,
        $.float_literal,
        $.string_literal,
        $.boolean_literal,
        $.parenthesized_expression,
        $.unary_expression,
        $.binary_expression,
        $.assignment_expression,
        $.cast_expression,
        $.call_expression,
        $.index_expression,
        $.field_expression,
        $.struct_expression,
        $.let_declaration,
        $.return_expression,
        $.yield_expression,
        $.break_expression,
        $.continue_expression,
        $._block_like,
      ),

    _block_like: ($) =>
      choice(
        $.block,
        $.if_expression,
        $.switch_expression,
        $.while_expression,
        $.loop_expression,
        $.do_expression,
        $.for_expression,
        $.for_clause_expression,
      ),

    scoped_identifier: ($) =>
      prec(PREC.postfix + 1, seq(field('path', $.identifier), '::', field('name', $.identifier))),

    at: (_) => '@',

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    unary_expression: ($) =>
      prec(PREC.unary, seq(field('operator', choice('-', '!', '*', '&', '&&')), field('operand', $._expression))),

    binary_expression: ($) => {
      const table = [
        [PREC.logical_or, '||'],
        [PREC.logical_and, '&&'],
        [PREC.bit_or, '|'],
        [PREC.bit_xor, '^'],
        [PREC.bit_and, '&'],
        [PREC.equality, choice('==', '!=')],
        [PREC.comparison, choice('<', '>', '<=', '>=')],
        [PREC.shift, choice('<<', '>>')],
        [PREC.additive, choice('+', '-')],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];
      return choice(
        ...table.map(([precedence, operator]) =>
          prec.left(
            precedence,
            seq(
              field('left', $._expression),
              field('operator', operator),
              field('right', $._expression),
            ),
          ),
        ),
      );
    },

    assignment_expression: ($) =>
      prec.right(
        PREC.assign,
        seq(
          field('left', $._expression),
          field(
            'operator',
            choice('=', '+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>='),
          ),
          field('right', $._expression),
        ),
      ),

    increment_expression: ($) =>
      prec(PREC.postfix, seq(field('operand', $._expression), field('operator', choice('++', '--')))),

    cast_expression: ($) =>
      prec.left(PREC.cast, seq(field('value', $._expression), 'as', field('type', $._type))),

    call_expression: ($) =>
      prec(PREC.postfix, seq(field('function', $._expression), field('arguments', $.arguments))),

    arguments: ($) => seq('(', commaSep($._expression), ')'),

    index_expression: ($) =>
      prec(PREC.postfix, seq(field('value', $._expression), '[', field('index', $._expression), ']')),

    field_expression: ($) =>
      prec(PREC.postfix, seq(field('value', $._expression), '.', field('field', $._field_identifier))),

    struct_expression: ($) =>
      seq(
        field('name', choice($._type_identifier, $.scoped_type_identifier)),
        field('body', $.field_initializer_list),
      ),

    field_initializer_list: ($) => seq('{', commaSep($.field_initializer), '}'),

    field_initializer: ($) =>
      seq(field('field', $._field_identifier), ':', field('value', $._expression)),

    return_expression: ($) => prec.right(seq('return', optional($._expression))),

    yield_expression: ($) => prec.right(seq('yield', $._expression)),

    break_expression: (_) => 'break',

    continue_expression: (_) => 'continue',

    // -------------------------------------------------------------------------
    // Blocks and control flow
    // -------------------------------------------------------------------------

    block: ($) => seq('{', repeat($._statement), optional($._expression), '}'),

    if_expression: ($) =>
      prec.right(
        seq(
          'if',
          field('condition', $._expression),
          field('consequence', $.block),
          optional(seq('else', field('alternative', choice($.block, $.if_expression)))),
        ),
      ),

    switch_expression: ($) =>
      seq('switch', field('value', $._expression), field('body', $.switch_body)),

    switch_body: ($) => seq('{', repeat($.case_clause), optional($.default_clause), '}'),

    case_clause: ($) => seq('case', field('pattern', $._pattern), field('body', $.block)),

    default_clause: ($) => seq('default', field('body', $.block)),

    while_expression: ($) =>
      seq('while', field('condition', $._expression), field('body', $.block)),

    loop_expression: ($) => seq('loop', field('body', $.block)),

    do_expression: ($) =>
      prec.right(
        seq('do', field('body', $.block), optional(seq('while', field('condition', $._expression)))),
      ),

    for_expression: ($) =>
      choice(
        seq('for', field('binding', $.identifier), 'in', field('iterable', $._expression), field('body', $.block)),
        seq(
          'for',
          '(',
          field('binding', $.identifier),
          'in',
          field('iterable', $._expression),
          ')',
          field('body', $.block),
        ),
      ),

    for_clause_expression: ($) =>
      seq(
        'for',
        '(',
        field('initializer', optional($._expression)),
        ';',
        field('condition', optional($._expression)),
        ';',
        field('update', optional(choice($._expression, $.increment_expression))),
        optional(seq(';', field('each', $.let_declaration))),
        ')',
        field('body', $.block),
      ),

    // -------------------------------------------------------------------------
    // Patterns
    // -------------------------------------------------------------------------

    _pattern: ($) =>
      choice($.variant_pattern, $.struct_variant_pattern, $._expression),

    // Preferred over a call where both fit, as at the top of a `case`
    variant_pattern: ($) =>
      prec.dynamic(
        1,
        seq(
          field('variant', choice($.identifier, $.scoped_identifier)),
          '(',
          commaSep($._sub_pattern),
          ')',
        ),
      ),

    struct_variant_pattern: ($) =>
      seq(
        field('variant', choice($.identifier, $.scoped_identifier)),
        '{',
        commaSep($.field_pattern),
        '}',
      ),

    field_pattern: ($) =>
      choice(
        field('name', $.binding),
        seq(field('field', $._field_identifier), ':', field('pattern', $._sub_pattern)),
      ),

    _sub_pattern: ($) =>
      choice(
        $.variant_pattern,
        $.struct_variant_pattern,
        $.binding,
        $.scoped_identifier,
        $.integer_literal,
        $.float_literal,
        $.string_literal,
        $.boolean_literal,
        seq('-', choice($.integer_literal, $.float_literal)),
      ),

    binding: ($) => $.identifier,

    // -------------------------------------------------------------------------
    // Names and literals
    // -------------------------------------------------------------------------

    identifier: (_) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    // Hidden, so the aliased identifier is the only node
    _type_identifier: ($) => alias($.identifier, $.type_identifier),

    _field_identifier: ($) => alias($.identifier, $.field_identifier),

    integer_literal: (_) =>
      token(
        seq(
          choice(/0[xX][0-9a-fA-F]+/, /0[bB][01]+/, /0[oO][0-7]+/, /[0-9]+/),
          optional(/[us](8|16|32|64)/),
        ),
      ),

    float_literal: (_) =>
      token(
        choice(
          seq(/[0-9]+\.[0-9]*([eE][+-]?[0-9]+)?/, optional(/f(32|64)/)),
          seq(/[0-9]+[eE][+-]?[0-9]+/, optional(/f(32|64)/)),
          seq(/[0-9]+/, /f(32|64)/),
        ),
      ),

    string_literal: ($) =>
      seq('"', repeat(choice($.escape_sequence, token.immediate(prec(1, /[^"\\\n]+/)))), token.immediate('"')),

    escape_sequence: (_) => token.immediate(/\\./),

    boolean_literal: (_) => choice('true', 'false'),
  },
});
