; Keywords
[
  "fn"
  "var"
  "const"
  "global"
  "class"
  "enum"
  "interface"
  "namespace"
  "if"
  "else"
  "while"
  "for"
  "switch"
  "case"
  "return"
  "break"
  "continue"
  "try"
  "catch"
  "defer"
  "errdefer"
  "true"
  "false"
  "pub"
  "mut"
] @keyword

; Types
[
  "i8" "i16" "i32" "i64" "i128" "iSize"
  "u8" "u16" "u32" "u64" "u128" "uSize"
  "f16" "f32" "f64" "f128"
  "bool" "void" "type" "anytype"
] @type.builtin

(primitive_type) @type.builtin
(user_type) @type
(type_arguments) @type

; Functions
(function_declaration
  name: (identifier) @function)
(call_expression
  function: (expression
    (identifier) @function))
(call_expression
  function: (expression
    (field_expression
      field: (identifier) @function.method)))

; Parameters
(parameter
  name: (identifier) @parameter)

; Variables
(variable_declaration
  name: (identifier) @variable)
(constant_declaration
  name: (identifier) @constant)
(global_declaration
  name: (identifier) @variable.global)

; Fields
(field_declaration
  name: (identifier) @property)
(field_expression
  field: (identifier) @property)

; Literals
(integer_literal) @number
(float_literal) @number
(string_literal) @string
(char_literal) @character
(boolean_literal) @boolean
(null_literal) @constant.builtin
(error_literal) @constant.builtin

; Comments
(comment) @comment

; Operators
[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|"
  "="
  "." "=>" ":" ";" "," "(" ")" "[" "]" "{" "}"
] @operator

; Punctuation
[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  "."
  ":"
  ";"
] @punctuation.delimiter

; Attributes
(attribute) @attribute

; Enum variants
(enum_variant
  name: (identifier) @constant)

; Namespace
(namespace_declaration
  name: (identifier) @namespace)

; Test
(test_declaration
  name: (identifier) @function.test)

; Special
"?" @operator
"!" @operator
"&" @operator
"*" @operator
