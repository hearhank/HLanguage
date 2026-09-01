; H language highlights — aligned with docs/SPEC/syntax (2026-08-30)

; ---------- Keywords (01 §1.2.1, 42 keywords) ----------
[
  "var" "const" "fn" "global"
  "if" "else" "while" "for" "break" "continue" "return" "switch" "defer" "errdefer"
  "class" "struct" "enum" "union" "tree" "interface" "where"
  "namespace" "import" "pub" "export"
  "owned" "move" "mut"
  "and" "or" "try" "catch" "orelse"
  "comptime" "anytype" "type"
  "async" "await"
  "extern"
  "void" "null" "true" "false"
  "as"
] @keyword
; 注：spawn 走普通调用形态（02 §2.1），以 @function.call 着色；无独立 token

; ---------- Builtin types (03 §3.2) ----------
[
  "i8" "i16" "i32" "i64" "i128" "isize"
  "u8" "u16" "u32" "u64" "u128" "usize"
  "f16" "f32" "f64"
  "bool" "comptime_int" "comptime_float"
] @type.builtin

(primitive_type) @type.builtin
(user_type) @type
(type_arguments) @type
(mut_type) @type
(owned_type) @type

; ---------- Functions ----------
(function_declaration
  name: (identifier) @function)
(extern_function_declaration
  name: (identifier) @function)
(method_signature
  name: (identifier) @function)

(call_expression
  function: (expression
    (identifier) @function.call))
(builtin_call
  name: (identifier) @function.builtin)

; test functions (12 §12.1)
(test_declaration
  name: (identifier) @function.test)

; ---------- Parameters & variables ----------
(parameter
  name: (identifier) @parameter)
(capture
  name: (identifier) @variable)
(variable_declaration
  name: (identifier) @variable)
(constant_declaration
  name: (identifier) @constant)
(global_declaration
  name: (identifier) @variable.global)

; ---------- Fields & properties ----------
(field_declaration
  name: (identifier) @property)
(field_expression
  field: (identifier) @property)
(field_initializer
  name: (identifier) @property)

; ---------- Literals ----------
(integer_literal) @number
(float_literal) @number
(string_literal) @string
(raw_string_literal) @string
(char_literal) @character
(escape_sequence) @string.escape
(boolean_literal) @boolean
(null_literal) @constant.builtin
(error_literal) @constant.builtin
(error_pattern) @constant.builtin

; ---------- Comments (01 §1.3) ----------
(doc_comment) @comment.doc
(comment) @comment

; ---------- Operators (02 §2.1) ----------
[
  "+" "-" "*" "/" "%" "%%" "**"
  "==" "!=" "<" ">" "<=" ">="
  "and" "or" "!" "~" "&" "|" "^" "<<" ">>"
  "&&" "||"
  ".."
  "=" "+=" "-=" "*=" "/=" "&=" "|=" "^="
  "?"
  "@" "=>"
] @operator

; ---------- Punctuation ----------
[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

[
  "," "." ":" ";"
] @punctuation.delimiter

; ---------- Attributes (04 §4.9) ----------
(attribute
  name: (identifier) @attribute)

; ---------- Enum variants & patterns ----------
(enum_variant
  name: (identifier) @constant)
(qualified_pattern
  variant: (identifier) @constant)

; ---------- Namespaces & labels ----------
(namespace_declaration
  name: (identifier) @namespace)
(break_statement
  label: (identifier) @label)
(continue_statement
  label: (identifier) @label)
