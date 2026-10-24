**Bug Fixes**

- `table<K, V>` fields and class parents with a container value type (e.g. `table<string, integer[]>`) no longer lose the nested element type.
- Writing to a member of a `---@type Foo` field (`ns.state.steps = ...`) no longer replaces the type `Foo` declares for it, and go-to-definition now goes to the `@field`.
