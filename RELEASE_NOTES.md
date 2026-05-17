**New**

- Union member narrowing based on field presence — `if info.title then` narrows a union to members where `title` is a required field ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/nil-safety.html))
- Boolean type-guard aliases — variables assigned a `type()` check result can be used as narrowing guards ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/type-guards.html))
- `invalid-op` diagnostic for the length (`#`) operator on unsupported types ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#invalid-op))
- Quick fix code actions — `@as` cast insertion for `type-mismatch` family, and diagnostic related information (jump to declaration)
- Snippet completions for common patterns (e.g. `if`/`for`/`function`)
- Generate-annotations completion — auto-generate `@param`/`@return` blocks for functions
- Function call snippet skips adding parens when cursor is already inside existing parens
- LSP type hierarchy support (supertypes and subtypes navigation)
- Selection range support (expand/shrink selection by semantic structure)
- Workspace diagnostics pull model for more efficient diagnostic delivery
- Semantic tokens range support (editor only requests tokens for visible lines)
- Incremental text sync (editor sends diffs instead of full file contents)
- TOC language server support (hover, completions, and diagnostics for `.toc` files) ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/toc-files.html))
- Cargo-fuzz targets for lexer, parser, and analysis

**Bug Fixes**

- Fix stale semantic tokens flashing during pending edits
- Fix inlay hints jumping to wrong positions during typing
- Fix `@diagnostic disable-line` not suppressing on the same line
- Fix cross-file annotation updates not propagating on edit
- Fix cross-file functions missing body-inferred return types
- Fix go-to-definition for fields resolving to wrong external file
- Fix `defclass` scan missing stubs context
- Fix `shadowed-local` false positive when outer variable is declared later in the file
- Fix `grouped-return-mismatch` false positive for union return types and forwarded correlated returns
- Fix `unknown-diag-code` false positive on custom `CODE_ALIASES`
- Fix false positive diagnostic in branch-assigned variables
- Fix field narrowing through `elseif` chains
- Fix narrowing for dynamic bracket-access conditions and ensure-initialized with variable keys
- Fix `assert` narrowing reverting multi-return sibling variable types
- Fix `inject-field` firing on `@class`-annotated variables (now suppressed)
- Fix `@class` overriding `@type` on same variable
- Fix correlated return inference for branch-merged locals and first inferred return tuple
- Fix inferred return types for tail-call delegating functions and single-path tail calls
- Fix forward-referenced unannotated functions showing wrong inferred type
- Fix generic type parameters leaking into resolved types
- Fix loop-carried variable type inference showing nil
- Fix bracket access producing spurious nil, redundant unions, or nonsensical types on named classes and string-literal-union keys
- Fix field inference leaking through interior bracket access
- Fix array element type inference leaking hash-table values from unions
- Fix `missing-parameter` false positive on variadic callback parameters
- Fix type annotation on table field with literal initializer
- Fix duplicate method in hover for class methods
- Fix Mixin hover returning no info
- Fix numeric for-loop counter showing `none` at declaration
- Fix table constructor completions for typed fields

**Improvements**

- Reduce hover latency on large files
- Improve parser error recovery for malformed syntax
- Show initial constructor type for bracket-mutated tables in hover
- Batch wiki fetching into a single request during stub generation
- Enrich Widget stub methods with wiki-scraped return types
- `nil-index` diagnostic changed to default-disabled (opt in via `.wowluarc.json`)
