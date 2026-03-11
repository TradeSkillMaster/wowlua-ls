# wowlua_ls — Future Work

Running document of deferred work items and future improvements.

---

## Annotations

- **@see** — Cross-reference links (37 uses in WoW stubs). Documentation-only, no type system impact.

---

## Diagnostics

### Low value

- **global-in-nil-env** — Lua 5.1 `setfenv` patterns.
- **doc-field-no-class** — `@field` without preceding `@class`. Simple but rare mistake.
- **undefined-doc-name** — References to undefined types in annotations. Moderate value.
- **unknown-cast-variable** — Casting undefined variables. Not applicable (we don't support `@cast`).
- **cast-type-mismatch** — Incompatible `@cast` types. Not applicable.
- **cast-local-type** — Local cast to different type. Not applicable.
- **empty-block** — Empty `if` / `while` blocks. Stylistic, low signal.
- **trailing-space** — Whitespace lint. Better handled by formatters.
- **unused-vararg** — Unused `...` in function body. Low value.
- **redundant-return** — `return` at end of function with no value. Stylistic.
- **newfield-call** / **newline-call** — Ambiguous multi-line table/call patterns. Rare.
- **ambiguity-1** — Operator precedence ambiguity. Very rare.
- **count-down-loop** — Decrementing for loop with wrong step sign. Rare.
- **no-unknown** — Strict mode: flag all untyped variables. Too noisy for addon dev.
- **codestyle-check** / **name-style-check** / **spell-check** — Formatting/style. Out of scope.
- **global-element** — Convention warning for undeclared globals. Overlaps with `undefined-global`.
- **incomplete-signature-doc** / **missing-global-doc** / **missing-local-export-doc** — Doc completeness. Out of scope.

---

## Type Inference

- **Backward type inference from body usage** — Currently, function parameter types are only determined by `@param` annotations or by accumulating a union of call-site argument types. Body-level usage (e.g. `param + 2` implies `number`, `param .. "x"` implies `string`) doesn't constrain the parameter type. Adding backward inference would let the type system derive constraints from operators and typed function calls within the body, enabling diagnostics when a call site passes an incompatible type (e.g. passing `string` to a function that does arithmetic on the param). This would also improve hover accuracy for unannotated params that are only used internally.

---

## Known Limitations

- **Reassignment overwrites hover type for earlier references** — Symbol versions lack positional awareness: if a variable is reassigned later in a block (e.g. `node = node.next` in a while loop), hover on earlier references shows the reassigned type rather than the version at that point. The nil-check diagnostic is correctly suppressed by narrowing, but hover displays the wrong (nullable) type.

- **Cross-file addon chains deeper than 3 parts** — The scanner handles `ns.X.Y = expr` (3-part chains) for addon namespace fields, but deeper chains like `ns.A.B.C = expr` are silently ignored. In practice WoW addon code doesn't use deeper chains at the top level.

---

## Type System

- **Class-type vs instance-type separation for LibTSMClass** — Currently the LS treats `@defclass`-created values as a single table type used for both the class object (with static methods) and instances (with instance methods). Libraries like LibTSMClass distinguish between a class table (which has static methods like `_ExtendStateSchema(cls)`, `_AddActionScripts(cls, ...)`, factory methods like `.Create(name)`) and instances of that class (which have instance methods like `:Acquire()`, `:__init()`). A proper solution would give the LS two faces for each class:
  1. The **class table** — holds static/factory methods where the first param is the class itself
  2. The **instance type** — holds instance methods where `self` is an instance

  This could be modeled via `@static` annotations on methods/fields, a separate `@class-meta` type, or by inferring from `__static` accessor patterns. Would improve type-checking accuracy for static method calls, constructor return types, and hover information. Currently worked around by the `is_method_call` fix for dot-defined functions called with colon syntax.

---

## WoW API Stubs

- **Flavor filtering** — Retail vs Classic API differentiation (bitmask data available in Ketho's repo).
