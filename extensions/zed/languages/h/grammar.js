/**
 * Tree-sitter grammar for H language
 *
 * This grammar defines the syntax rules for H language,
 * used for syntax highlighting and code folding in Zed editor.
 */

module.exports = grammar({
  name: 'h',

  // Whitespace and comments
  extras: $ => [
    /\s/,
    $.comment,
  ],

  // Conflict resolution
  conflicts: $ => [
    [$.expression, $.statement],
    // `Foo(...)` is ambiguous: a call expression (expression position)
    // vs a user type with type arguments (type position). Prefer the
    // expression (call) interpretation in expression contexts.
    [$.expression, $.user_type],
    // `error.X` is both an error literal and a field access on `error`.
    [$.error_literal, $.field_expression],
    // A single parenthesized expression `(x)` is both a call argument list
    // and (potentially) a tuple. Prefer the argument-list reading.
    [$.argument_list, $.tuple_expression],
    // `import A.B.{x}`: the dotted module path is ambiguous with a bare
    // identifier followed by a field access. Prefer the module-path reading.
    // The self-conflict lets GLR explore both greedy and non-greedy module
    // path readings; the non-greedy one fails (no `{` follows the early `.`)
    // and gets pruned.
    [$.module_path, $.field_expression],
    [$.module_path],
  ],

  // Rules
  rules: {
    // Entry point
    source_file: $ => repeat($.declaration),

    // Comments
    comment: $ => choice(
      seq('//', /.*/),
      seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/'),
    ),

    // Declarations
    declaration: $ => choice(
      $.import_declaration,
      $.function_declaration,
      $.variable_declaration,
      $.constant_declaration,
      $.global_declaration,
      $.class_declaration,
      $.enum_declaration,
      $.interface_declaration,
      $.namespace_declaration,
      $.test_declaration,
    ),

    // Import declaration
    //   import H.std.{io};
    //   import H.std.{io as my};
    //   import H.std.net.{http, tcp};
    import_declaration: $ => seq(
      'import',
      field('module', $.module_path),
      '.',
      '{',
      commaSep($.import_item),
      '}',
      ';',
    ),

    // Module path: a sequence of identifiers separated by dots (e.g. H.std.net)
    module_path: $ => seq(
      $.identifier,
      repeat(seq('.', $.identifier)),
    ),

    // A single imported item, optionally renamed with `as` (e.g. io as my)
    import_item: $ => seq(
      field('name', $.identifier),
      optional(seq('as', field('alias', $.identifier))),
    ),

    // Function declaration
    function_declaration: $ => seq(
      optional('pub'),
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameter_list)),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      field('body', $.block),
    ),

    // Generic function type parameters: `fn swap<T>(a: *mut T, b: *mut T) void`
    type_parameter_list: $ => seq(
      '<',
      commaSep1($.type_parameter),
      '>',
    ),

    type_parameter: $ => field('name', $.identifier),

    parameter_list: $ => seq(
      '(',
      commaSep($.parameter),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      ':',
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
    ),

    // Variable declaration
    variable_declaration: $ => seq(
      'var',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type))),
      '=',
      field('value', $.expression),
      ';',
    ),

    // Constant declaration
    constant_declaration: $ => seq(
      'const',
      field('name', $.identifier),
      optional(seq(':', field('type', $.type))),
      '=',
      field('value', $.expression),
      ';',
    ),

    // Global declaration
    global_declaration: $ => seq(
      'global',
      field('name', $.identifier),
      ':',
      field('type', $.type),
      '=',
      field('value', $.expression),
      ';',
    ),

    // Class declaration
    class_declaration: $ => seq(
      optional('pub'),
      optional(field('attribute', $.attribute)),
      'class',
      field('name', $.identifier),
      field('body', $.class_body),
    ),

    class_body: $ => seq(
      '{',
      repeat($.field_declaration),
      '}',
    ),

    field_declaration: $ => seq(
      optional('pub'),
      field('name', $.identifier),
      ':',
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
      ',',
    ),

    // Enum declaration
    enum_declaration: $ => seq(
      optional('pub'),
      'enum',
      field('name', $.identifier),
      field('body', $.enum_body),
    ),

    enum_body: $ => seq(
      '{',
      commaSep($.enum_variant),
      '}',
    ),

    enum_variant: $ => seq(
      field('name', $.identifier),
      optional(field('payload', $.type)),
    ),

    // Interface declaration
    interface_declaration: $ => seq(
      optional('pub'),
      'interface',
      field('name', $.identifier),
      optional(seq(':', commaSep1($.type))),
      field('body', $.interface_body),
    ),

    interface_body: $ => seq(
      '{',
      repeat($.method_signature),
      '}',
    ),

    method_signature: $ => seq(
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      field('return_type', $.type),
      ',',
    ),

    // Namespace declaration
    namespace_declaration: $ => seq(
      'namespace',
      field('name', $.identifier),
      field('body', $.namespace_body),
    ),

    namespace_body: $ => seq(
      '{',
      repeat($.declaration),
      '}',
    ),

    // Test declaration
    test_declaration: $ => seq(
      '[test]',
      optional(seq('(', field('name', $.string_literal), ')')),
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      field('return_type', $.type),
      field('body', $.block),
    ),

    // Attribute
    attribute: $ => seq(
      '[',
      field('name', $.identifier),
      optional(seq('(', field('value', $.expression), ')')),
      ']',
    ),

    // Types
    type: $ => choice(
      $.primitive_type,
      $.user_type,
      $.pointer_type,
      $.optional_type,
      $.owned_type,
      $.error_union_type,
      $.array_type,
      $.slice_type,
      $.function_type,
      $.tuple_type,
    ),

    primitive_type: $ => choice(
      'i8', 'i16', 'i32', 'i64', 'i128', 'iSize',
      'u8', 'u16', 'u32', 'u64', 'u128', 'uSize',
      'f16', 'f32', 'f64', 'f128',
      'bool', 'void', 'type', 'anytype',
    ),

    user_type: $ => seq(
      field('name', $.identifier),
      optional(field('type_arguments', $.type_arguments)),
    ),

    type_arguments: $ => seq(
      '<',
      commaSep1($.type),
      '>',
    ),

    pointer_type: $ => seq(
      choice('*', '*mut'),
      field('type', $.type),
    ),

    optional_type: $ => seq(
      '?',
      field('type', $.type),
    ),

    // Owned type:  `owned T` (H ownership syntax; e.g. owned Vec(String)).
    // Unlike `*T` (shared/read-only) and `*mut T` (mutable),  `owned T`
    // means the value is owned.
    owned_type: $ => seq(
      'owned',
      field('type', $.type),
    ),

    error_union_type: $ => prec.right(1, seq(
      optional(field('error_set', $.type)),
      '!',
      field('type', $.type),
    )),

    array_type: $ => seq(
      '[',
      field('size', $.expression),
      ']',
      field('type', $.type),
    ),

    slice_type: $ => seq(
      '&',
      optional('mut'),
      '[',
      field('type', $.type),
      ']',
    ),

    function_type: $ => seq(
      'fn',
      field('parameters', $.parameter_list),
      field('return_type', $.type),
    ),

    tuple_type: $ => seq(
      '(',
      commaSep1($.type),
      ')',
    ),

    // Statements
    statement: $ => choice(
      $.expression_statement,
      $.variable_declaration,
      $.constant_declaration,
      $.while_statement,
      $.for_statement,
      $.return_statement,
      $.break_statement,
      $.continue_statement,
      $.catch_statement,
      $.defer_statement,
      $.errdefer_statement,
      $.block,
    ),

    expression_statement: $ => seq(
      $.expression,
      ';',
    ),

    while_statement: $ => seq(
      'while',
      field('condition', $.expression),
      field('body', $.block),
    ),

    for_statement: $ => seq(
      'for',
      field('iterator', $.expression),
      '|',
      field('variable', $.identifier),
      '|',
      field('body', $.block),
    ),

    switch_case: $ => seq(
      'case',
      field('pattern', $.switch_pattern),
      '=>',
      field('body', choice($.expression, $.block)),
      ',',
    ),

    switch_pattern: $ => choice(
      $.identifier,
      $.literal,
      '_',
    ),

    return_statement: $ => seq(
      'return',
      optional(field('value', $.expression)),
      ';',
    ),

    break_statement: $ => seq(
      'break',
      optional(seq(':', field('label', $.identifier))),
      ';',
    ),

    continue_statement: $ => seq(
      'continue',
      optional(seq(':', field('label', $.identifier))),
      ';',
    ),

    catch_statement: $ => seq(
      'catch',
      field('variable', $.identifier),
      field('body', $.block),
    ),

    defer_statement: $ => seq(
      'defer',
      field('body', $.block),
    ),

    errdefer_statement: $ => seq(
      'errdefer',
      field('body', $.block),
    ),

    block: $ => seq(
      '{',
      repeat($.statement),
      '}',
    ),

    // Expressions
    expression: $ => choice(
      $.literal,
      $.identifier,
      $.binary_expression,
      $.unary_expression,
      $.call_expression,
      $.field_expression,
      $.index_expression,
      $.array_expression,
      $.tuple_expression,
      $.if_expression,
      $.switch_expression,
      $.block_expression,
      $.closure_expression,
      $.await_expression,
      $.try_expression,
      $.catch_expression,
      $.orelse_expression,
      $.unwrap_expression,
    ),

    binary_expression: $ => choice(
      prec.left(1, seq(field('left', $.expression), '||', field('right', $.expression))),
      prec.left(2, seq(field('left', $.expression), '&&', field('right', $.expression))),
      prec.left(3, seq(field('left', $.expression), choice('==', '!=', '<', '>', '<=', '>='), field('right', $.expression))),
      prec.left(4, seq(field('left', $.expression), choice('+', '-'), field('right', $.expression))),
      prec.left(5, seq(field('left', $.expression), choice('*', '/', '%'), field('right', $.expression))),
    ),

    unary_expression: $ => prec.right(7, choice(
      seq('!', field('operand', $.expression)),
      seq('-', field('operand', $.expression)),
      seq('*', field('operand', $.expression)),
      seq('&', field('operand', $.expression)),
    )),

    call_expression: $ => prec(9, seq(
      field('function', $.expression),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      commaSep($.expression),
      ')',
    ),

    field_expression: $ => prec(9, seq(
      field('object', $.expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(9, seq(
      field('object', $.expression),
      '[',
      field('index', $.expression),
      ']',
    )),

    array_expression: $ => seq(
      '[',
      commaSep($.expression),
      ']',
    ),

    tuple_expression: $ => seq(
      '(',
      commaSep1($.expression),
      ')',
    ),

    field_initializer: $ => seq(
      field('name', $.identifier),
      '=',
      field('value', $.expression),
    ),

    if_expression: $ => prec.right(seq(
      'if',
      field('condition', $.expression),
      field('consequence', choice($.expression, $.block)),
      optional(seq('else', field('alternative', choice($.expression, $.block)))),
    )),

    switch_expression: $ => seq(
      'switch',
      field('value', $.expression),
      '{',
      commaSep($.switch_case),
      '}',
    ),

    block_expression: $ => seq(
      '{',
      repeat($.statement),
      field('value', $.expression),
      '}',
    ),

    closure_expression: $ => seq(
      'fn',
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      field('body', $.block),
    ),

    await_expression: $ => prec(9, seq(
      field('expression', $.expression),
      '.',
      'await',
    )),

    try_expression: $ => seq(
      'try',
      field('expression', $.expression),
    ),

    catch_expression: $ => prec.left(8, seq(
      field('expression', $.expression),
      'catch',
      field('default', $.expression),
    )),

    orelse_expression: $ => prec.left(8, seq(
      field('expression', $.expression),
      '??',
      field('default', $.expression),
    )),

    unwrap_expression: $ => prec(9, seq(
      field('expression', $.expression),
      '.',
      '?',
    )),

    // Literals
    literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.char_literal,
      $.boolean_literal,
      $.null_literal,
      $.error_literal,
    ),

    integer_literal: $ => /\d+(_?\d)*/,

    float_literal: $ => /\d+\.\d+/,

    string_literal: $ => seq(
      '"',
      repeat(choice($.string_content, $.escape_sequence)),
      '"',
    ),

    string_content: $ => /[^"\\]+/,

    escape_sequence: $ => seq('\\', choice('n', 't', 'r', '\\', '"', '0')),

    char_literal: $ => seq(
      "'",
      choice($.char_content, $.escape_sequence),
      "'",
    ),

    char_content: $ => /[^'\\]/,

    boolean_literal: $ => choice('true', 'false'),

    null_literal: $ => 'null',

    error_literal: $ => seq(
      'error',
      '.',
      field('name', $.identifier),
    ),

    // Identifier
    identifier: $ => /[a-zA-Z_][a-zA-Z0-9_]*/,
  },
});

// Helper functions
function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
