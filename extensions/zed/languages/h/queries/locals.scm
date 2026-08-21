; Local variables and scopes

; Scopes
(function_declaration) @local.scope
(block) @local.scope
(closure_expression) @local.scope

; Definitions
(variable_declaration
  name: (identifier) @local.definition)
(constant_declaration
  name: (identifier) @local.definition)
(parameter
  name: (identifier) @local.definition)
(for_statement
  variable: (identifier) @local.definition)
(catch_statement
  variable: (identifier) @local.definition)

; References
(identifier) @local.reference
