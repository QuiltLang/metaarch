; Inject the fragment language's grammar into annotated `impl` bodies:
; `impl get /hello rust↖ … ↗` highlights the interior as Rust. Un-annotated
; fragments default to the service's `lang`, but a query can't express that
; cross-node lookup — `metaarch-lsp` covers them with semantic tokens instead.

((impl_entry
   language: (fragment_language (identifier) @injection.language)
   body: (fragment (fragment_text) @injection.content))
 (#set! injection.combined))

((comment) @injection.content
 (#set! injection.language "comment"))
