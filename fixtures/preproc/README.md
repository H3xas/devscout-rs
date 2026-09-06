# preproc fixtures

All fixtures below are fully synthetic; no identifiers come from a real
codebase. Each file exercises how the preprocessor pre-pass (which blanks
inactive `#if`/`#else` arms and directive lines before parsing) interacts
with extraction, with no build symbols defined.

| File | What it exercises |
| --- | --- |
| preproc_chain_interrupt_if.cs | A `#if DEBUG` directive interrupts a fluent method chain; the inactive arm is blanked before parsing so the chain reads as one statement, and the calls before and after it keep their real line numbers in ascending order. |
| preproc_chain_interrupt_ifelse.cs | A `#if`/`#else` group interrupts a fluent chain with a real call on each arm; only the active arm's call reaches the parser, and neither call produces a reference of its own — only the trailing argument does. |
| preproc_chain_interrupt_nested_control.cs | A `#if` group nested inside another `#if` group interrupts a fluent chain; when the outer arm is inactive, the entire nested group is blanked regardless of the inner symbol. |
| preproc_chain_wholestmt_control.cs | A `#if` block wraps a whole statement rather than interrupting an expression; when inactive, the guarded statement is blanked and produces no reference, while the statement before it is untouched. |
| preproc_namespace_selection.cs | A file's namespace and `using` directive are chosen by a preprocessor symbol, mirroring the convention of compiling one source file into two namespaces; only the active arm's namespace and members are indexed, and the inactive arm's `using` directive is not recorded. |
