### New

- **`@alias (opaque)` for nominal primitive types** — create type-safe aliases that prevent accidental mixing of same-shaped types ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html#opaque-aliases))
- **XML frame/template scanning** — automatically extract class declarations and globals from addon `.xml` files: virtual templates, non-virtual frames, `parentKey`/`parentArray` children, `inherits`/`mixin` chains ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/xml-scanning.html))
- **`@enum (key)` annotation modifier** — declare enums where the keys are the semantic values ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/classes.html#key-based-enums-enum-key))
- **Implicit generics for pass-through return params** — functions that return a parameter directly infer the generic relationship without explicit `@generic` ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html))
- **Variadic generics and `@narrows-arg` for Mixin functions** — `@generic T, ...M` collects excess args into an intersection; `@narrows-arg` mutates argument types in-place ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html#variadic-generics))
- **`unbalanced-assignments` diagnostic** — warn when destructuring more variables than a function returns ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#unbalanced-assignments))
- **`field-type-mismatch` in typed table constructors** — catch mismatched field values when constructing `@class` tables inline
- **Tuple-union narrowing for `pcall`/`pcallwithenv`** — `ok, result` patterns now narrow the error/success branches ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/multi-return.html))
- **Nil narrowing through `(x or 0) > 0`** — common Lua idiom now properly narrows nil
- **Infer nil return for functions that never return** — functions with no return statements infer nil rather than unknown
- **`invalid-class-parent` diagnostic** — warn on subclassing basic data types like `string`, `number`, `boolean`
- **Intersection types in `expression<>` context** — expression strings support `expression<A & B, R>` ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/expressions.html))
- **Cross-file namespace propagation from local call returns** — namespace fields assigned from function calls now resolve their types across files
- **`select(2, ...)` resolves to addon private table** — the common addon init pattern is now understood
- **LuaLS-style vararg return syntax** — `@return type ...` is now accepted alongside the native `@return ...type` form
- **FrameXML global type inference** — infer types for FrameXML globals from constructor calls, enum refs, and variable references
- **Comma-separated template lists in CreateFrame** — `` CreateFrame`Frame, Tmpl1, Tmpl2` `` now works

### Bug Fixes

- Fix narrowing override blocking re-narrowing after variable reassignment
- Fix overload resolution with `any`-typed params causing false positives
- Fix ambiguous overload return type resolution picking wrong candidate
- Fix false positive `type-mismatch` on variables reassigned in if/else branches
- Fix nil initialization contaminating type after complete if/else coverage
- Fix nil-stripping narrowing persisting after variable reassignment
- Fix false positive `nil-index` on narrowed variables
- Fix false positive `undefined-field` on frame properties ([#45](https://github.com/tradeskillmaster/wowlua-ls/issues/45)), enum classes, and manual mixin annotations ([#42](https://github.com/tradeskillmaster/wowlua-ls/issues/42))
- Fix `type-mismatch` false positive on `select()` with `returns<F>` vararg projection
- Fix accessor hover lost when `@class` overlays defclass parents
- Fix RHS type propagation for deep defclass hierarchies
- Fix class table fields showing `any` for function call values
- Fix table shape loss on namespace field assignment
- Fix cross-file self-field scan when variable name differs from class name
- Fix diagnostic flicker and debounce timer during active typing
- Fix go-to-definition fallback for field-position tokens
- Fix off-by-one in code lens usage count
- Fix table constructor field key incorrectly showing global hover
- Fix missing return types for classic-only `C_*` wiki-scraped stubs
- Fix JetBrains crash on out-of-bounds offset ([#37](https://github.com/tradeskillmaster/wowlua-ls/issues/37))
- Fix comment coloring in JetBrains ([#36](https://github.com/tradeskillmaster/wowlua-ls/issues/36))
- Suppress completions on keyword tokens

### Improvements

- Optimize startup with mimalloc allocator and parallel stub loading
- Optimize type resolution fixpoint loop performance
- Reduce LSP reprocessing during typing (fewer re-analyses on keystroke)
- Shorten nil unions to `T?` in hover and inlay hints for readability
- Prioritize locals over globals in scope completions
- Deduplicate identical inferred return type overloads in hover
- Highlight function-typed fields as function tokens in expression strings
- Add hover for `@accessor` tokens
- Color return labels as parameters in hover grammar
- Replace Ketho Wiki.lua with direct wiki scraping for more complete classic stubs
