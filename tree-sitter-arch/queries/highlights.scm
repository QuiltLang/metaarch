; arch highlights — nvim-flavored (specific patterns first, catch-all last).

[
  "system"
  "service"
  "lang"
  "port"
  "db"
  "emits"
  "consumes"
  "impl"
  "table"
  "enum"
] @keyword

(pk) @attribute

(system name: (identifier) @namespace)
(service name: (identifier) @type)
(table name: (identifier) @type)
(emits_entry event: (identifier) @type)
(consumes_entry event: (identifier) @type)

(field name: (identifier) @property)
(type_name (identifier) @type.builtin)
(enum_type (identifier) @constant)

(lang_entry value: (identifier) @constant)
(method (identifier) @function)
(fragment_language (identifier) @constant)

(path) @string
(number) @number
(comment) @comment

[
  "↖"
  "↗"
  "{"
  "}"
  "("
  ")"
] @punctuation.bracket

[
  ":"
  ","
] @punctuation.delimiter
