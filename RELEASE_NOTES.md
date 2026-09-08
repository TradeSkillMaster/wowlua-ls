**Bug Fixes**
- `params<>` event-handler typing now works for register-by-name libraries that pass the addon object as an argument (typed `keyof T`) rather than as `self`: the string handler name is typed and go-to-definition/rename reach it, and its payload-projected parameters no longer trip `missing-param-annotation` or `unused-function`. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/events.html))
- `@meta` declaration files no longer report `unused-function` for the functions they declare.

**Improvements**
- Function-level `@generic` type parameters now flow into inline callback parameters, bound from sibling arguments — e.g. a `fun(value: V, index: K)` callback is typed from a `table<K, V>` argument.
- Boolean-literal table fields now widen to `boolean` in inferred types, matching numeric and string literals, so structurally identical constructors converge to one shape instead of surfacing as `{ok: true} | {ok: false}`.
