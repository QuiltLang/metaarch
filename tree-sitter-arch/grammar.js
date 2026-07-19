/**
 * @file arch grammar for tree-sitter — editor-facing syntax for metaarch's
 * `.arch` DSL (docs/wiki/plan.md, phase 4). Deliberately loose, like the
 * quilt-side `metaarch-lang` parser: any identifier parses as a type, method,
 * or language — the closed sets and topology checks stay in
 * `metaarch-parser`/`metaarch-spec`, which own diagnostics. This grammar only
 * has to segment the file for highlighting and injections.
 * @license MIT OR Apache-2.0
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

/**
 * One or more `rule`s separated by `sep`, with an optional trailing `sep`.
 * @param {RuleOrLiteral} rule
 * @param {RuleOrLiteral} sep
 */
function sepTrailing(rule, sep) {
  return seq(rule, repeat(seq(sep, rule)), optional(sep));
}

module.exports = grammar({
  name: 'arch',

  extras: $ => [/\s/, $.comment],

  word: $ => $.identifier,

  rules: {
    source_file: $ => seq($.system, repeat($.service)),

    system: $ => seq('system', field('name', $.identifier)),

    service: $ =>
      seq(
        'service',
        field('name', $.identifier),
        '{',
        repeat($._entry),
        '}',
      ),

    _entry: $ =>
      choice(
        $.lang_entry,
        $.port_entry,
        $.db_entry,
        $.emits_entry,
        $.consumes_entry,
        $.impl_entry,
      ),

    lang_entry: $ => seq('lang', field('value', $.identifier)),

    port_entry: $ => seq('port', field('value', $.number)),

    db_entry: $ =>
      seq(
        'db',
        field('engine', $.identifier),
        '{',
        repeat($.table),
        '}',
      ),

    table: $ =>
      seq(
        'table',
        field('name', $.identifier),
        '{',
        optional(sepTrailing($.field, ',')),
        '}',
      ),

    field: $ =>
      seq(
        field('name', $.identifier),
        ':',
        field('type', $._type),
        optional($.pk),
      ),

    pk: _ => 'pk',

    _type: $ => choice($.enum_type, $.type_name),

    type_name: $ => $.identifier,

    enum_type: $ =>
      seq('enum', '(', sepTrailing($.identifier, ','), ')'),

    emits_entry: $ =>
      seq(
        'emits',
        field('event', $.identifier),
        '{',
        optional(sepTrailing($.field, ',')),
        '}',
      ),

    consumes_entry: $ => seq('consumes', field('event', $.identifier)),

    // impl := "impl" method path [lang] "↖" … "↗" — the optional language
    // annotation is the same spelling a `.arch.quilt` quote uses (`rust↖`).
    impl_entry: $ =>
      seq(
        'impl',
        field('method', $.method),
        field('path', $.path),
        optional(field('language', $.fragment_language)),
        field('body', $.fragment),
      ),

    method: $ => $.identifier,

    fragment_language: $ => $.identifier,

    // The brackets nest (a fragment may itself quote); the interior is opaque
    // here — `metaarch-lsp` re-parses it with the fragment language's own
    // grammar, and the injection query hands it to editors the same way.
    fragment: $ =>
      seq('↖', repeat(choice($.fragment_text, $.fragment)), '↗'),

    fragment_text: _ => token(prec(-1, /[^↖↗]+/)),

    path: _ => /\/[A-Za-z0-9_\-/]*/,

    number: _ => /[0-9]+/,

    comment: _ => token(seq('#', /[^\n]*/)),

    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
  },
});
