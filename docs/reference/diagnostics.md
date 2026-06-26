# All Diagnostics

Complete reference of every diagnostic code. For an introduction to how diagnostics work and how to configure them, see the [Diagnostics guide](/guide/diagnostics).

## Warning severity

| Code | Description |
|---|---|
| `deprecated` | Usage of `@deprecated` symbols |
| `discard-returns` | Ignoring `@nodiscard` return values |
| `type-mismatch` | Argument type vs `@param` mismatch (including generic type arguments, e.g. `Container<number>` vs `Container<boolean>`, and string-literal unions, e.g. `"x"` against `"A"\|"B"\|"C"`) |
| `return-mismatch` | Return type vs `@return` mismatch |
| `field-type-mismatch` | Field assignment vs `@field` type mismatch |
| `assign-type-mismatch` | Reassignment vs `@type` mismatch |
| `generic-constraint-mismatch` | Generic argument doesn't satisfy a class, alias, or `keyof` constraint |
| `param-constraint-mismatch` | Method called when the receiver's `@requires` type-param constraint isn't satisfied |
| `missing-parameter` | Missing required function arguments |
| `redundant-parameter` | Extra function arguments |
| `missing-return-value` | Return with fewer values than `@return` |
| `redundant-return-value` | Return with more values than `@return` |
| `grouped-return-mismatch` | Return values don't match any tuple-union `@return` case |
| `missing-return` | Function missing return statement |
| `undefined-global` | Reference to unresolved global name |
| `undefined-field` | Accessing nonexistent field on a `@class` or a module-private table (`local p = {}`) |
| `need-check-nil` | Field/method access on possibly-nil value **(off by default)** |
| `nil-index` | Bracket-indexing a table with a possibly-nil key **(off by default)** |
| `nil-table-key` | Table key type annotation includes nil (`table<string?, V>`) |
| `access-private` | Accessing `@field private` from outside |
| `access-protected` | Accessing `@field protected` from outside hierarchy (also `_`-prefixed fields when `implicit_protected_prefix` is enabled) |
| `duplicate-index` | Duplicate keys in table constructors |
| `redundant-value` | Extra values in assignments |
| `unbalanced-assignments` | More variables than values in assignments, including when a function call returns fewer values than variables assigned |
| `missing-fields` | Missing required fields when constructing `@class` tables |
| `undefined-doc-class` | Undefined class name in `@class Foo: Parent` |
| `undefined-doc-name` | Undefined type name in annotations |
| `undefined-doc-param` | `@param` name not matching function parameters |
| `duplicate-doc-param` | Duplicate `@param` annotations |
| `duplicate-doc-field` | Duplicate `@field` annotations |
| `duplicate-doc-alias` | Duplicate `@alias` declarations |
| `doc-field-no-class` | `@field` on a non-`@class` table |
| `doc-func-no-function` | Function-level annotation (`@param`, `@return`, etc.) not attached to a function definition |
| `circle-doc-class` | Circular `@class` inheritance chains |
| `malformed-annotation` | Unknown or incomplete `---@` annotations |
| `multi-return-projection` | `returns<F>` discards extra return values from F |
| `builds-field-not-self` | `@builds-field` method uses `@return ClassName` instead of `@return self` |
| `unknown-diag-code` | Unknown code in `@diagnostic` directives |
| `duplicate-constructor` | Multiple `@constructor` on a single class |
| `constructor-return` | `@constructor` with return other than `@return self` |
| `count-down-loop` | For-loop step direction doesn't match start/end |
| `wrong-flavor-api` | API not available in all declared flavors |
| `redundant-class-generic` | Method redeclares class-level `@generic` |
| `cannot-call` | Calling a value whose type is not callable |
| `invalid-op` | Operator applied to incompatible types (e.g. `+` on strings instead of `..`) **(off by default)** |
| `create-global` | Implicit global creation |
| `invalid-class-parent` | Inheriting from a non-table type (`number`, `string`, literals, etc.) |
| `mixed-enum-values` | `@enum` with mixed number/string values or unsupported value types |
| `unknown-callback-event` | Event name passed to a callback registry that was never declared via `GenerateCallbackEvents` **(off by default)** |

## Hint severity

| Code | Description |
|---|---|
| `return-self-class-name` | Method uses `@return ClassName` instead of `@return self` |
| `unused-local` | Unreferenced local variables |
| `unused-function` | Unused function definitions **(off by default)** |
| `unused-vararg` | Function declares `...` but never uses it **(off by default)** |
| `redefined-local` | Same-scope local variable redefinition |
| `shadowed-local` | Local variable shadows an outer-scope variable |
| `inject-field` | Setting undeclared fields on `@class` tables |
| `duplicate-set-field` | Setting an already-set field on `@class` tables |
| `unreachable-code` | Code after return |
| `code-after-break` | Code after break |
| `incomplete-signature-doc` | Partial `@param`/`@return` annotations **(off by default)** |
| `missing-param-annotation` | Non-file-local function parameter has no `@param` **(off by default)** |
| `missing-return-annotation` | Non-file-local function returns a value but has no `@return` **(off by default)** |
| `empty-block` | Empty control flow body |
| `redundant-return` | Bare `return` at end of function |
| `trailing-space` | Line ends with whitespace |
| `not-precedence` | `not x <cmp> y` is `(not x) <cmp> y` |
| `redundant-or` | `or` where left side is always truthy (RHS is dead code) **(off by default)** |
| `redundant-and` | `and` where left side is always falsy (RHS is dead code) or always truthy (operator is a no-op) **(off by default)** |
| `redundant-condition` | `if`/`elseif`/`while` condition is [provably constant](#redundant-condition) **(off by default)** |
| `implicit-nil-return` | Bare `return` in function with optional `@return` **(off by default)** |
| `unknown-param-type` | Parameter type can't be inferred **(off by default)** |
| `unknown-return-type` | Return value has no resolvable type **(off by default)** |
| `unknown-local-type` | Local assignment has unknown type **(off by default)** |
| `unknown-field-type` | Field assignment has unknown type **(off by default)** |

### `missing-param-annotation` / `missing-return-annotation`

Flag functions that lack `@param` / `@return` documentation. Both are off by default — enable them when you want every public function fully documented:

```json
{
  "diagnostics": {
    "enable": ["missing-param-annotation", "missing-return-annotation"]
  }
}
```

`missing-param-annotation` fires once per source parameter without a matching `@param` (the implicit `self` of a colon method and the conventional `_` throwaway are skipped). `missing-return-annotation` fires once on a function whose body returns a value but has no `@return`.

**Scope — only functions reachable beyond their file are checked.** Checked:

- ✅ Global functions: `function GlobalFn() end`
- ✅ Methods/fields on a global table: `function GlobalApi.Run() end`
- ✅ Methods/fields on a `@class`: `function Widget:SetValue() end`
- ✅ Methods/fields on the addon namespace: `function ns.Module.Process() end`
- ✅ Methods on a local table that is attached to the addon namespace (`local M = {}; function M.foo() end; ns.M = M`)

Skipped:

- ❌ `local function` definitions
- ❌ Anonymous function literals (`local f = function() … end`, callback arguments)
- ❌ Bare `function foo()` that reassigns a forward-declared `local foo`
- ❌ **Methods on a file-private local table** (`local cache = {}; function cache.get() end`) that never escapes the file

The escape test for table methods reuses the workspace global scan — a method is "reachable" exactly when the language server registers it as a cross-file symbol, so this stays consistent with go-to-definition and find-references.

This differs from `incomplete-signature-doc`, which fires only when a signature is *partially* annotated (and regardless of where the function lives). `missing-*-annotation` fires even on a completely undocumented function, but only for non-file-local ones. The two can be enabled together; a partially-annotated global will then report under both.

### `redundant-condition`

Flags `if`/`elseif`/`while`/`repeat...until` conditions that are provably always true or always false. Detected patterns:

- **Always-truthy/falsy type** — the condition's resolved type is guaranteed truthy (e.g. `table`, `number`) or guaranteed falsy (`nil`).
- **Negation of a constant** — `not expr` where `expr` is itself always truthy or always falsy (e.g. `if not tbl` where `tbl` is a table).
- **Type-incompatible equality** — `==`/`~=` between values whose types can never match at runtime (e.g. `num == "hello"`, `nonNilVar == nil`).
- **Literal-union miss** — `x == "c"` where `x` is typed as `"a"|"b"` and `"c"` is not a member.
- **Two-literal comparison** — both sides are concrete literals (e.g. `1 == 2`, `"a" == "a"`, `3 < 2`).
- **Self-comparison** — `x < x` or `x > x` (always false; NaN-safe). `<=`/`>=`/`==`/`~=` self-comparisons are excluded because NaN breaks them.
- **Redundant `type()` guard** — `type(x) == "number"` where `x` is already known to be a `number` (always true) or can never be a `number` (always false).

Loop idioms (`while true`, `repeat...until false`) are not flagged. Conditions referencing variables reassigned inside loops are suppressed to avoid false positives.

### `unused-function`

Flags function definitions that are never referenced anywhere in the workspace. Covers both top-level global functions and methods defined on tables (e.g. `function NS.Method()`).

**What counts as "used":**

- Called directly via `call_resolutions` (handles deep type inference, self-calls, etc.)
- Referenced as a value (e.g. passed as a callback, stored in a variable) — detected via field-access token resolution with inheritance

**What is skipped (not flagged):**

- Functions whose name starts with `_` (convention for intentionally unused)
- Functions defined in library files (directories marked with `library` in `.wowluarc.json`)
- Interface methods — if 2+ distinct tables define the same method name, the method is assumed to be a framework callback pattern (duck-typing dispatch)
- Inherited methods — if a parent class's field is referenced, child overrides are also considered used

**Cross-file behavior:**

This diagnostic requires a multi-file workspace scan. It compares definitions from all files against references from all files. In single-file mode (e.g. `evaluate`), only per-file unused functions are detected.

## TOC file diagnostics

These diagnostics apply to `.toc` files only. See the [TOC Files guide](/guide/toc-files) for details.

| Code | Severity | Description |
|---|---|---|
| `toc-missing-interface` | Warning | Required `## Interface:` field is missing |
| `toc-duplicate-header` | Warning | Same header key appears more than once |
| `toc-unknown-header` | Hint | Header not in the known catalog and not `X-*` |
| `toc-invalid-interface` | Error | Interface value is not a valid numeric version |
| `toc-nonexistent-file` | Warning | Referenced file does not exist on disk |
| `toc-invalid-value` | Warning | Value doesn't match expected format |

## LuaLS compatibility aliases

| Alias | Maps to |
|---|---|
| `invisible` | `access-private`, `access-protected` |
| `param-type-mismatch` | `type-mismatch` |
| `return-type-mismatch` | `return-mismatch` |

Diagnostic codes that LuaLS defines but wowlua_ls has no equivalent for (e.g.
`lowercase-global`, `cast-type-mismatch`, `unused-label`) are accepted silently
in `---@diagnostic` directives — they suppress nothing here, but won't trip
`unknown-diag-code`, so a project that also runs LuaLS can keep its suppressions
without noise.
