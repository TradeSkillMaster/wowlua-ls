# All Diagnostics

Complete reference of every diagnostic code. For an introduction to how diagnostics work and how to configure them, see the [Diagnostics guide](/guide/diagnostics).

## Warning severity

| Code | Description |
|---|---|
| `deprecated` | Usage of `@deprecated` symbols |
| `discard-returns` | Ignoring `@nodiscard` return values |
| `type-mismatch` | Argument type vs `@param` mismatch |
| `return-mismatch` | Return type vs `@return` mismatch |
| `field-type-mismatch` | Field assignment vs `@field` type mismatch |
| `assign-type-mismatch` | Reassignment vs `@type` mismatch |
| `generic-constraint-mismatch` | Generic argument doesn't satisfy class constraint |
| `missing-parameter` | Missing required function arguments |
| `redundant-parameter` | Extra function arguments |
| `missing-return-value` | Return with fewer values than `@return` |
| `redundant-return-value` | Return with more values than `@return` |
| `grouped-return-mismatch` | Return values don't match any tuple-union `@return` case |
| `missing-return` | Function missing return statement |
| `undefined-global` | Reference to unresolved global name |
| `undefined-field` | Accessing nonexistent field on `@class` |
| `need-check-nil` | Field/method access on possibly-nil value **(off by default)** |
| `nil-index` | Bracket-indexing a table with a possibly-nil key |
| `access-private` | Accessing `@field private` from outside |
| `access-protected` | Accessing `@field protected` from outside hierarchy (also `_`-prefixed fields when `implicit_protected_prefix` is enabled) |
| `duplicate-index` | Duplicate keys in table constructors |
| `redundant-value` | Extra values in assignments |
| `unbalanced-assignments` | More variables than values in assignments |
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
| `create-global` | Implicit global creation |

## Hint severity

| Code | Description |
|---|---|
| `return-self-class-name` | Method uses `@return ClassName` instead of `@return self` |
| `unused-local` | Unreferenced local variables |
| `unused-function` | Unused function definitions |
| `unused-vararg` | Function declares `...` but never uses it **(off by default)** |
| `redefined-local` | Same-scope local variable redefinition |
| `inject-field` | Setting undeclared fields on `@class` tables |
| `duplicate-set-field` | Setting an already-set field on `@class` tables |
| `unreachable-code` | Code after return |
| `code-after-break` | Code after break |
| `incomplete-signature-doc` | Partial `@param`/`@return` annotations **(off by default)** |
| `empty-block` | Empty control flow body |
| `redundant-return` | Bare `return` at end of function |
| `trailing-space` | Line ends with whitespace |
| `not-precedence` | `not x <cmp> y` is `(not x) <cmp> y` |
| `implicit-nil-return` | Bare `return` in function with optional `@return` **(off by default)** |
| `unknown-param-type` | Parameter type can't be inferred **(off by default)** |
| `unknown-return-type` | Return value has no resolvable type **(off by default)** |
| `unknown-local-type` | Local assignment has unknown type **(off by default)** |
| `unknown-field-type` | Field assignment has unknown type **(off by default)** |

## LuaLS compatibility aliases

| Alias | Maps to |
|---|---|
| `invisible` | `access-private`, `access-protected` |
| `param-type-mismatch` | `type-mismatch` |
| `return-type-mismatch` | `return-mismatch` |
