/**
 * Tree-sitter grammar for H language
 *
 * Authority: docs/SPEC/syntax/ (00-index.md + 13 module files, 2026-08-30)
 * Reference implementation: tag1/hc/src/parser/
 *
 * Aligned with: 01 (lexical/declarations), 02 (operators/control flow),
 * 03 (types), 04 (extended types), 05 (functions), 06 (interfaces),
 * 08 (errors), 10 (meta), 11 (concurrency), 12 (testing).
 */

module.exports = grammar({
  name: 'h',

  // Prefer keyword literals over the identifier word token
  word: $ => $.identifier,

  extras: $ => [
    /\s/,
    $.comment,
    $.doc_comment,
  ],

  conflicts: $ => [
    [$.expression, $.statement],
    [$.expression, $.user_type],
    [$.error_literal, $.field_expression],
    [$.argument_list, $.tuple_expression],
    [$.module_path, $.field_expression],
    [$.module_path],
    // `Foo { ... }`: typed literal vs block expression
    [$.struct_literal, $.block],
    // `Vec<i32>[1, 2]`: container literal vs index expression
    [$.container_literal, $.index_expression],
    // `Type.variant` in switch patterns vs field access
    [$.qualified_pattern, $.field_expression],
    // `[test(...)] fn` — dedicated test rule vs generic attribute + fn
    [$.test_declaration, $.function_declaration],
    // `mut T` as a type vs expression position keyword
    [$.mut_type, $.expression],
    // parenthesized type tuple vs parenthesized expression
    [$.tuple_type, $.parenthesized_expression],
    // closure `mut |x|` prefix stack vs `mut T` writable-value type
    [$.mut_type, $.closure_expression],
    // `owned T` type prefix in expression position (J1: box 返回 owned T)
    [$.owned_type, $.expression],
    // `Chan<i32>.init` 泛型类型表达式 vs `<` 比较（02 §2.18 类型名单）
    [$.generic_type_expression, $.binary_expression],
    // 可选返回类型 vs 下一声明的 fn 型返回（罕见形态，交 GLR 按上下文取舍）
    [$.method_signature],
    [$.function_declaration],
    // 控制流语句体后的块边界（GLR 按上下文取舍）
    [$.while_statement],
    [$.for_statement],
    [$.if_statement],
  ],

  rules: {
    source_file: $ => repeat($._declaration),

    // ---------- Comments (01 §1.3) ----------
    comment: $ => token(seq('//', /.*/)),
    doc_comment: $ => token(seq('///', /.*/)),
    _block_comment: $ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),

    // ---------- Declarations (01 §1.8, 09 §9.2) ----------
    _declaration: $ => choice(
      $.import_declaration,
      $.comptime_block,
      $.function_declaration,
      $.extern_function_declaration,
      $.variable_declaration,
      $.constant_declaration,
      $.global_declaration,
      $.class_declaration,
      $.struct_declaration,
      $.union_declaration,
      $.tree_declaration,
      $.enum_declaration,
      $.interface_declaration,
      $.namespace_declaration,
      $.test_declaration,
    ),

    // import (09 §9.3): whole module / symbol select / file include (.hs)
    import_declaration: $ => seq(
      'import',
      choice($.module_path, field('file', $.string_literal)),
      optional(seq('.', '{', commaSep($.import_item), '}')),
      optional(seq('as', field('alias', $.identifier))),
      ';',
    ),

    module_path: $ => seq(
      $.identifier,
      repeat(seq('.', $.identifier)),
    ),

    import_item: $ => seq(
      field('name', $.identifier),
      optional(seq('as', field('alias', $.identifier))),
    ),

    // Function (05 §5.1): [pub] [export] [attrs] [async] fn name<T>(params) Ret where ... {}
    function_declaration: $ => seq(
      repeat($.attribute),
      optional('pub'),
      optional('export'),
      optional('async'),
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameter_list)),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      optional(field('where_clause', $.where_clause)),
      field('body', $.block),
    ),

    // extern fn (05 §5.7, ADR-0020): pure declaration, ';' terminated
    extern_function_declaration: $ => seq(
      optional('pub'),
      'extern',
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', $.type_parameter_list)),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      ';',
    ),

    type_parameter_list: $ => seq(
      '<',
      commaSep1(field('name', $.identifier)),
      '>',
    ),

    where_clause: $ => seq(
      'where',
      commaSep1(seq(field('param', $.identifier), ':', field('constraint', $.type))),
    ),

    parameter_list: $ => seq(
      '(',
      commaSep($.parameter),
      ')',
    ),

    // Parameter (05 §5.2, K1/ADR-0036): [var [mut]] [owned] name: Type [= default]
    parameter: $ => seq(
      optional(seq('var', optional('mut'))),
      optional('owned'),
      field('name', $.identifier),
      ':',
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
    ),

    // Variable (01 §1.9): var [mut] name [: Type] [= init];  + tuple destructure
    variable_declaration: $ => seq(
      'var',
      optional('mut'),
      choice(
        field('name', $.identifier),
        seq('(', commaSep1(field('name', $.identifier)), ')'),
      ),
      optional(seq(':', field('type', $.type))),
      optional(seq('=', field('value', $.expression))),
      ';',
    ),

    // Constant (01 §1.10): init required
    constant_declaration: $ => seq(
      'const',
      optional('pub'),
      field('name', $.identifier),
      optional(seq(':', field('type', $.type))),
      '=',
      field('value', $.expression),
      ';',
    ),

    // Global (01 §1.11, D4): init optional (zero-value)
    global_declaration: $ => seq(
      'global',
      optional('pub'),
      field('name', $.identifier),
      optional(seq(':', field('type', $.type))),
      optional(seq('=', field('value', $.expression))),
      ';',
    ),

    // class (04 §4.1): heap reference type — fields + methods
    class_declaration: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'class',
      field('name', $.identifier),
      optional(seq(':', commaSep1(field('interface', $.type)))),
      field('body', $.class_body),
    ),

    class_body: $ => seq(
      '{',
      repeat(choice($.field_declaration, $.function_declaration)),
      '}',
    ),

    // Field (04 §4.1/§4.2, G3/K1): [attrs] [pub] [owned] name: Type [= default]
    // Note: no `mut` name-prefix (G3); writability via `mut T` type form
    field_declaration: $ => seq(
      repeat($.attribute),
      optional('pub'),
      optional('owned'),
      field('name', $.identifier),
      ':',
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
      optional(','),
    ),

    // enum (04 §4.3): variant `name` or `name: Type`; names may be keywords
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
      field('name', choice($.identifier, 'null', 'true', 'false')),
      optional(seq(':', field('payload', $.type))),
    ),

    // struct (04 §4.2, ADR-0022): continuous value type, fields only,
    // field defaults, field-level [Align(n)]
    struct_declaration: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'struct',
      field('name', $.identifier),
      field('body', $.struct_body),
    ),

    struct_body: $ => seq(
      '{',
      repeat($.field_declaration),
      '}',
    ),

    // union (04 §4.4, ADR-0014 K1): untagged, fields only
    union_declaration: $ => seq(
      optional('pub'),
      'union',
      field('name', $.identifier),
      field('body', $.union_body),
    ),

    union_body: $ => seq(
      '{',
      repeat($.field_declaration),
      '}',
    ),

    // tree (04 §4.5, G2): reserved keyword, parses like class (pending feature)
    tree_declaration: $ => seq(
      repeat($.attribute),
      optional('pub'),
      'tree',
      field('name', $.identifier),
      field('body', $.class_body),
    ),

    // interface (04 §4.3 → 06 §6.1): method contracts, ';' terminated
    interface_declaration: $ => seq(
      optional('pub'),
      'interface',
      field('name', $.identifier),
      optional(seq(':', commaSep1(field('super', $.type)))),
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
      optional(field('type_parameters', $.type_parameter_list)),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      optional(field('where_clause', $.where_clause)),
      optional(';'),
    ),

    // namespace (09 §9.2)
    namespace_declaration: $ => seq(
      optional('pub'),
      'namespace',
      field('name', $.identifier),
      field('body', $.namespace_body),
    ),

    namespace_body: $ => seq(
      '{',
      repeat($._declaration),
      '}',
    ),

    // test (12 §12.1): [test] [test("name")] [test(async)] [test(thread)]
    // [test(timeout=N)] [test("name", async, timeout=N)]
    test_declaration: $ => seq(
      '[',
      'test',
      optional(seq('(', commaSep(choice($.string_literal, 'async', 'thread', seq('timeout', '=', $.integer_literal))), ')')),
      ']',
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      field('body', $.block),
    ),

    // Attribute (04 §4.9, ADR-0022 §5): [name] [name(arg,...)] [name{field = v}]
    attribute: $ => seq(
      '[',
      field('name', $.identifier),
      optional(choice(
        seq('(', commaSep($.attribute_argument), ')'),
        seq('{', commaSep($.field_initializer), '}'),
      )),
      ']',
    ),

    attribute_argument: $ => $.expression,

    // comptime block (10 §10.4): declaration-level, load-time eval
    comptime_block: $ => seq(
      'comptime',
      field('body', $.block),
    ),

    // ---------- Types (03 §3.1) ----------
    type: $ => choice(
      $.primitive_type,
      $.user_type,
      $.pointer_type,
      $.reference_type,
      $.mut_type,
      $.optional_type,
      $.owned_type,
      $.error_union_type,
      $.array_type,
      $.tuple_type,
      $.function_type,
    ),

    // Primitives (03 §3.2): isize/usize correct case; comptime_*; f128 ❌ (F1)
    primitive_type: $ => choice(
      'i8', 'i16', 'i32', 'i64', 'i128', 'isize',
      'u8', 'u16', 'u32', 'u64', 'u128', 'usize',
      'f16', 'f32', 'f64',
      'bool', 'void', 'type', 'anytype',
      'comptime_int', 'comptime_float',
    ),

    user_type: $ => seq(
      field('name', $.identifier),
      optional(field('type_arguments', $.type_arguments)),
    ),

    type_arguments: $ => seq(
      '<',
      commaSep1(choice($.type, $.integer_literal)),
      '>',
    ),

    // Pointer (03 §3.4): *T / *mut T — prec: `*mut` 相邻优先于 `*` + `mut T`
    pointer_type: $ => prec(2, seq(
      '*',
      optional('mut'),
      field('type', $.type),
    )),

    // Reference/slice (03 §3.3): &T / &mut T / &[T] / &mut [T] — prec 同 pointer_type
    reference_type: $ => prec(2, seq(
      '&',
      optional('mut'),
      choice(
        seq('[', field('slice', $.type), ']'),
        field('referent', $.type),
      ),
    )),

    // `mut T` writable-value type form (K1/ADR-0036)
    mut_type: $ => seq(
      'mut',
      field('type', $.type),
    ),

    // `?T` (03 §3.5)
    optional_type: $ => seq(
      '?',
      field('type', $.type),
    ),

    // `owned T` type prefix (01 §1.9.2, 03 §3.1)
    owned_type: $ => seq(
      'owned',
      field('type', $.type),
    ),

    // `!T` / `E!T` (03 §3.6)
    error_union_type: $ => prec.right(1, seq(
      optional(field('error_set', $.type)),
      '!',
      field('type', $.type),
    )),

    // `[N]T` fixed array (03 §3.3) — prec above literal (vs array literal `[1, 2]`)
    array_type: $ => prec(2, seq(
      '[',
      field('size', choice($.integer_literal, $.identifier)),
      ']',
      field('type', $.type),
    )),

    tuple_type: $ => seq(
      '(',
      commaSep1($.type),
      ')',
    ),

    // `fn(params) Ret` call contract type (03 §3.8; FnN named form via user_type)
    // prec.right: greedy return type — `fn() !T` consumes the error union
    function_type: $ => prec.right(seq(
      'fn',
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
    )),

    // ---------- Statements & control flow (02 §2.11–2.17) ----------
    statement: $ => choice(
      $.expression_statement,
      $.variable_declaration,
      $.constant_declaration,
      $.if_statement,
      $.while_statement,
      $.for_statement,
      $.switch_expression,
      $.return_statement,
      $.break_statement,
      $.continue_statement,
      $.defer_statement,
      $.errdefer_statement,
      $.block,
    ),

    expression_statement: $ => seq(
      $.expression,
      ';',
    ),

    // Labeled loops (02 §2.16): `:label while (...)` / `:label for (...)`
    _label: $ => seq(':', field('label', $.identifier)),

    if_statement: $ => prec.right(2, seq(
      'if',
      '(',
      field('condition', $.expression),
      ')',
      optional(field('capture', $.capture)),
      field('then', $._block_or_statement),
      optional(seq(
        'else',
        choice($.if_statement, field('alternative', $._block_or_statement)),
      )),
    )),

    while_statement: $ => seq(
      optional($._label),
      'while',
      '(',
      field('condition', $.expression),
      ')',
      optional(field('capture', $.capture)),
      optional(seq(':', '(', field('step', $.expression), ')')),
      optional(field('capture2', $.capture)),
      field('body', $._block_or_statement),
    ),

    for_statement: $ => seq(
      optional($._label),
      'for',
      '(',
      field('iterator', $.expression),
      ')',
      field('capture', $.capture),
      field('body', $._block_or_statement),
    ),

    // Capture (02 §2.14): |x| / |mut x| / |move x| — prec: 优先于同形态闭包
    capture: $ => prec(1, seq(
      '|',
      optional(choice('mut', 'move')),
      field('name', $.identifier),
      '|',
    )),

    _block_or_statement: $ => choice(prec(1, $.block), $.statement),

    return_statement: $ => seq(
      'return',
      optional(field('value', $.expression)),
      ';',
    ),

    break_statement: $ => seq(
      'break',
      optional($._label),
      ';',
    ),

    continue_statement: $ => seq(
      'continue',
      optional($._label),
      ';',
    ),

    defer_statement: $ => seq(
      'defer',
      field('body', $.expression),
      ';',
    ),

    errdefer_statement: $ => seq(
      'errdefer',
      field('body', $.expression),
      ';',
    ),

    block: $ => seq(
      '{',
      repeat($.statement),
      '}',
    ),

    // ---------- Expressions (02 §2.1–2.10) ----------
    expression: $ => choice(
      $.literal,
      $.identifier,
      $.generic_type_expression,
      $.assignment_expression,
      $.binary_expression,
      $.unary_expression,
      $.call_expression,
      $.builtin_call,
      $.field_expression,
      $.index_expression,
      $.deref_expression,
      $.unwrap_expression,
      $.array_expression,
      $.tuple_expression,
      $.struct_literal,
      $.container_literal,
      $.if_expression,
      $.switch_expression,
      $.block,
      $.closure_expression,
      $.await_expression,
      $.try_expression,
      $.catch_expression,
      $.orelse_expression,
      $.move_expression,
      $.parenthesized_expression,
      $.control_flow_expression,
    ),

    // Assignment (02 §2.7): statement-level; targets = ident/index/field/deref
    assignment_expression: $ => prec.right(1, seq(
      field('target', choice($.identifier, $.index_expression, $.field_expression, $.deref_expression)),
      choice('=', '+=', '-=', '*=', '/=', '&=', '|=', '^='),
      field('value', $.expression),
    )),

    // Binary precedence (02 §2.1): or < and < .. < compare < | < ^ < & < shift < +- < */%% < **(right)
    binary_expression: $ => choice(
      ...[
        [1, choice('or', '||')],
        [2, choice('and', '&&')],
        [3, '..'],
        [4, choice('==', '!=', '<', '>', '<=', '>=')],
        [5, '|'],
        [6, '^'],
        [7, '&'],
        [8, choice('<<', '>>')],
        [9, choice('+', '-')],
        [10, choice('*', '/', '%', '%%')],
      ].map(([precedence, operator]) =>
        prec.left(precedence, seq(
          field('left', $.expression),
          field('operator', operator),
          field('right', $.expression),
        )),
      ).concat([
        // `**` power (02 §2.2.1, H2): right-associative, above mul
        prec.right(11, seq(
          field('left', $.expression),
          field('operator', '**'),
          field('right', $.expression),
        )),
      ]),
    ),

    // Unary (02 §2.1 layer 12): - ! ~ & &mut move try await
    unary_expression: $ => prec.right(12, choice(
      seq('-', field('operand', $.expression)),
      seq('!', field('operand', $.expression)),
      seq('~', field('operand', $.expression)),
      seq('&', field('operand', $.expression)),
      seq('&', 'mut', field('operand', $.expression)),
      $.move_expression,
      $.try_expression,
      $.await_expression,
    )),

    move_expression: $ => prec.right(12, seq(
      'move',
      field('operand', $.expression),
    )),

    try_expression: $ => prec.right(12, seq(
      'try',
      field('expression', $.expression),
    )),

    await_expression: $ => prec.right(12, seq(
      'await',
      field('expression', $.expression),
    )),

    call_expression: $ => prec(14, seq(
      field('function', $.expression),
      field('arguments', $.argument_list),
    )),

    // `Chan<i32>.init(...)` — 表达式位置的泛型类型实例化（02 §2.18）
    generic_type_expression: $ => prec(1, seq(
      field('name', $.identifier),
      field('type_arguments', $.type_arguments),
    )),

    // `@name(args)` builtin call (13 §13.1) — call form required
    builtin_call: $ => prec(14, seq(
      '@',
      field('name', $.identifier),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      commaSep($.expression),
      ')',
    ),

    field_expression: $ => prec(14, seq(
      field('object', $.expression),
      '.',
      field('field', $.identifier),
    )),

    index_expression: $ => prec(14, seq(
      field('object', $.expression),
      '[',
      commaSep1(field('index', $.expression)),
      ']',
    )),

    // `p.*` deref (02 §2.9)
    deref_expression: $ => prec(14, seq(
      field('object', $.expression),
      '.*',
    )),

    // `x.?` unwrap (02 §2.9, E2: only `.?`)
    unwrap_expression: $ => prec(14, seq(
      field('object', $.expression),
      '.',
      '?',
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

    parenthesized_expression: $ => prec(2, seq(
      '(',
      $.expression,
      ')',
    )),

    field_initializer: $ => seq(
      field('name', $.identifier),
      '=',
      field('value', $.expression),
    ),

    // `Type{ field = v, ... }` typed literal (02 §2.9, 04)
    struct_literal: $ => prec(13, seq(
      field('type', $.type),
      '{',
      commaSep($.field_initializer),
      '}',
    )),

    // `Vec<i32>[1, 2, 3]` container literal (ADR-0027)
    container_literal: $ => prec(13, seq(
      field('type', $.user_type),
      '[',
      commaSep($.expression),
      ']',
    )),

    if_expression: $ => prec.right(2, seq(
      'if',
      '(',
      field('condition', $.expression),
      ')',
      optional(field('capture', $.capture)),
      field('consequence', choice($.expression, $.block)),
      'else',
      field('alternative', choice($.expression, $.block)),
    )),

    // switch (02 §2.15): patterns [if guard] => [|cap|] body; no `case` keyword
    switch_expression: $ => seq(
      'switch',
      '(',
      field('value', $.expression),
      ')',
      '{',
      repeat($.switch_arm),
      '}',
    ),

    switch_arm: $ => prec(1, seq(
      commaSep1($.switch_pattern),
      optional(seq('if', field('guard', $.expression))),
      '=>',
      optional(field('capture', $.capture)),
      field('body', choice($.block, $.expression)),
      optional(','),
    )),

    switch_pattern: $ => prec(1, choice(
      $.error_pattern,
      $.qualified_pattern,
      $.literal,
      $.identifier,
      'else',
      'null',
      'true',
      'false',
    )),

    // `error.Name` pattern (08 §8.5) — prec: switch 模式优先于字面量
    error_pattern: $ => prec(2, seq(
      'error',
      '.',
      field('name', $.identifier),
    )),

    // `Type.variant` pattern — variant may be a keyword (04 §4.3)
    qualified_pattern: $ => seq(
      field('type', $.identifier),
      '.',
      field('variant', choice($.identifier, 'null', 'true', 'false')),
    ),

    block: $ => seq(
      '{',
      repeat($.statement),
      '}',
    ),

    // Closure (05 §5.6): |v, w| expr / mut |v| {} / move |v| {} — prefixes stackable;
    // params ≥ 1（实现约束：`||` 为逻辑或别名，非零参闭包）
    closure_expression: $ => seq(
      repeat(choice('mut', 'move')),
      '|',
      commaSep1(field('param', $.identifier)),
      '|',
      choice(
        prec(1, field('body', $.block)),
        field('expr', $.expression),
      ),
    ),

    // orelse/catch postfix (02 §2.10); control-flow bailout RHS
    orelse_expression: $ => prec.left(13, seq(
      field('expression', $.expression),
      'orelse',
      field('default', choice($.expression, $.control_flow_expression)),
    )),

    catch_expression: $ => prec.left(13, seq(
      field('expression', $.expression),
      'catch',
      field('handler', choice(
        seq('|', field('err', $.identifier), '|', choice($.block, $.expression)),
        field('default', choice($.expression, $.control_flow_expression)),
      )),
    )),

    // `orelse return e` / `catch break` control-flow bailout forms (02 §2.10)
    control_flow_expression: $ => prec.right(seq(
      choice('return', 'break', 'continue'),
      optional(field('value', $.expression)),
    )),

    // ---------- Literals (01 §1.4–1.6) ----------
    literal: $ => choice(
      $.integer_literal,
      $.float_literal,
      $.string_literal,
      $.raw_string_literal,
      $.char_literal,
      $.boolean_literal,
      $.null_literal,
      $.error_literal,
    ),

    // Bases 0x/0b/0o (case-insensitive), `_` separators, width suffix iN/uN/fN/isize/usize
    integer_literal: $ => /(0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*)([iu](8|16|32|64|128|size)|[fF](32|64))?/,

    // Requires digit after '.', exponent e/E±, f32/f64 suffix
    // token(prec) ：词法级压过 integer（避免 `0.0` 被拆成 `0` + `.`）；注意后缀组整体可选
    float_literal: $ => token(prec(1, /\d[\d_]*\.\d[\d_]*([eE][+-]?\d[\d_]*)?([fF](32|64))?/)),

    string_literal: $ => seq(
      '"',
      repeat(choice($.string_content, $.escape_sequence)),
      '"',
    ),

    string_content: $ => /[^"\\]+/,

    escape_sequence: $ => choice(
      /\\[nrt\\"']/,
      /\\x[0-9a-fA-F]{2}/,
      /\\u\{[0-9a-fA-F_]+\}/,
    ),

    // Raw multiline string (01 §1.6.2): """...""" — no escapes
    raw_string_literal: $ => token(/"""[\s\S]*?"""/),

    char_literal: $ => seq(
      "'",
      choice($.escape_sequence, /[^'\\]/),
      "'",
    ),

    boolean_literal: $ => choice('true', 'false'),

    null_literal: $ => 'null',

    error_literal: $ => seq(
      'error',
      '.',
      field('name', $.identifier),
    ),

    // Identifier (01 §1.1.2): Unicode letters/digits + '_'
    identifier: $ => /[\p{L}_][\p{L}\p{N}_]*/u,
  },
});

// Helper functions
function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
