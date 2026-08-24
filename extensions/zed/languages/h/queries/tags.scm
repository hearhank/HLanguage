; Tags for code navigation and symbol search

; Function definitions
(function_declaration
  name: (identifier) @name) @definition.function

; Class definitions
(class_declaration
  name: (identifier) @name) @definition.class

; Enum definitions
(enum_declaration
  name: (identifier) @name) @definition.enum

; Interface definitions
(interface_declaration
  name: (identifier) @name) @definition.interface

; Namespace definitions
(namespace_declaration
  name: (identifier) @name) @definition.namespace

; Variable definitions
(variable_declaration
  name: (identifier) @name) @definition.variable

; Constant definitions
(constant_declaration
  name: (identifier) @name) @definition.constant

; Global definitions
(global_declaration
  name: (identifier) @name) @definition.variable

; Test definitions
(test_declaration
  name: (identifier) @name) @definition.function
