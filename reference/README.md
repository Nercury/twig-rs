# Reference

Facts that the code does not describe. Every statement here is about the pinned target in [`target.md`](target.md)
and is backed by upstream source (file:line) or by an oracle run. When code absorbs a fact (ported function plus
tests), the corresponding text can shrink to a pointer.

| Document | Contents |
|---|---|
| [target.md](target.md) | Pinned versions, oracle PHP settings, Twig defaults, upgrade procedure, path placeholders |
| [twig/syntax.md](twig/syntax.md) | Lexer (verbatim regexes, states, escapes, whitespace control), token stream, parser, expression grammar and precedence table, every tag, syntax errors and suggestions, gap analysis of the 2017 code |
| [twig/runtime.md](twig/runtime.md) | Compiled-template structure, context passing, for-loop/inheritance/macro/include/embed/trait runtime, every node's compiled form, getAttribute, node visitors, environment, error model, loaders |
| [twig/core-extension.md](twig/core-extension.md) | Every core filter/function/test/operator callable with signatures and port-ready behaviour, escaper strategies, sandbox, dump, template_from_string |
| [twig/test-suite.md](twig/test-suite.md) | `.test` fixture format and harness behaviour, DATA/CONFIG catalogue, fixture classification, PHPUnit test portability, oracle protocol |
| [php/engine-semantics.md](php/engine-semantics.md) | PHP 8.5 casts, numeric strings, float-to-string, arithmetic, comparison, array keys, var_dump/var_export formats, diagnostics catalogue |
| [php/stdlib.md](php/stdlib.md) | Every PHP builtin Twig calls, mapped to php-src with semantics, edge cases, error texts and porting notes |
