### New

- **`keyof T` constraint and `T[K]` indexed access types** — restrict a generic parameter to the field names of a class, and use `T[K]` to look up the corresponding field type. String literal completions are provided for keyof-constrained parameters. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html#keyof-constraints))
- **`redundant-condition` diagnostic** — warns when an `if`/`elseif`/`while` condition is always truthy or always falsy (off by default). ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics.html))
- **`redundant-or` / `redundant-and` diagnostics** — warn when the left side of `or` is always truthy or `and` is always falsy, making the right side dead code (off by default). ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/diagnostics.html))
- **Generic type argument checking at call sites** — parameterized class type arguments are now validated against their constraints when instantiated.
- **Inline event-callback typing** — anonymous callbacks passed to `RegisterEvent`-style calls now have their parameters and varargs typed from the bound event payload.
- **Event name hover** — hovering over an event name string in `RegisterEvent` calls shows the event's payload signature.
- **Nil-coalesce quick fix for `invalid-op`** — a code action to wrap a nilable operand in an `or` default.
- **Refactor: combine `@return` lines** — code action to merge multiple `@return` annotations into a single-line tuple form.
- **Bound generics in hover and inlay hints** — method hover and inlay hints now show concrete generic bindings (e.g. `Publisher<string>` instead of `Publisher<T>`).
- **`@overload` with `self<R>` and generic callback inference** — overloads can now use re-parameterized self returns with callback-inferred generics.
- **Number-literal tuple-union returns** — tuple-union `@return` cases can use number literals as sentinel discriminators (e.g. `@return (0, nil) | (number, string)`).

### Bug Fixes

- Fix false positive `missing-fields` on incrementally-built tables (fields added after construction).
- Fix false positive `invalid-op` on logical `or`/`and`, conditional concat expressions, `assert`-narrowed values, `type()` guards in `and`-expressions, and multi-term `or`-chain guards.
- Fix false positive `type-mismatch` on optional generic param after nil arg, self-referential multi-assign, and narrowed unions in compound conditions.
- Fix false positive `cannot-call` for callable class with renamed local variable.
- Fix false positive diagnostic on builder chain method calls, method chains, and `@cast` in `elseif` branches.
- Fix false positive diagnostic on narrowed union in for-loop.
- Fix cascading nil from recursive type inference.
- Fix early-exit type guard leaking into `elseif` body scopes.
- Fix generic binding from `fun(): T?` return type and false negative in generic type-arg variance check.
- Fix `NonNil` (`T!`) not stripping nil inside unions in `@return self<T!|V>`.
- Fix cross-file tuple-union sibling narrowing false positive.
- Fix boolean guard narrowing in inferred return cases.
- Fix hover showing nil for `and`/`or` alias versions and missing hover on self-referential field method chains.
- Fix deferred sibling narrowing for multi-return reassignments.
- Fix `@return` description coloring after non-letter `@`.
- Fix `fun()` type coloring inside union type annotations.
- Fix annotation completions falling back to global names; add missing annotation tag completions.
- Fix inferred return showing nil instead of `string?`.
- Fix duplicate `@diagnostic disable` comments in quick actions.
- Merge return types from all union members in method calls.

### Improvements

- **Fix LSP freeze on large workspaces** — diagnostic warm-up now runs on a background thread with bounded concurrency.
- **Speed up go-to-definition for stub symbols.**
- **Speed up workspace re-analysis after declaration edits.**
- **Speed up stub generation** with persistent caching.
- Infer return types for FrameXML factory functions in stub generation.
- Preserve local function signature when assigned to namespace field.
- Narrow correlated locals through `and` guards and after mutual early-exit guards.
- Add guard-implication narrowing for `if A and not B then return` patterns.
- Narrow nilable field in `(field or 0) > 0` guards.
- Strip nil from numeric-comparison guard symbol.
- Translate ancestor type-params through parameterized-parent bindings.
- Isolate diagnostic-affecting `.wowluarc.json` settings from parent configs.
- Deduplicate array and table types in union display.
- Disable word-based completion fallback for Lua in VS Code.
- Skip trailing optional params in completion snippets.
- Add `loadstring` tuple-union return override.
