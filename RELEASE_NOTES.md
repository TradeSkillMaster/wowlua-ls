**New**

- `@noreturn` marks a function that never returns because it always raises an error. A call to it ends its block the way `error()` does: after `if not player then Fail("no player") end`, `player` is narrowed, and a function ending in such a call doesn't get `missing-return` ([docs](https://tradeskillmaster.github.io/wowlua-ls/reference/annotations.html)).

**Improvements**

- A field read in another file now has the type that the file writing it infers. This covers fields of the addon namespace, global tables, `@class`es and mixins, including:
  - fields written in initializer functions, event handlers, methods and constructors
  - saved variables
  - tables nested inside a constructor (`Hub = { Libs = { Main = GetLib("X") } }`)
- A function called from another file keeps the signature its own file infers: the implicit `self`, backward-inferred parameter types and return types. Other files show this signature in hover and signature help and check their arguments against it. A parameter given a default (`name = name or "x"`, `if not name then name = "x" end`) is now inferred optional.
- WoW API stubs refreshed from the latest wow-ui-source and BlizzardInterfaceResources snapshots.

**Bug Fixes**

- A generic class's field, read through a type that binds the class's parameters, now has the bound type, in other files too. Examples: `opts.option.value` where `option: Value<boolean>`, or a field of a subclass of `Value<string>`.
- `@correlated` field groups now narrow the fields of classes declared in other files, and of their subclasses.
- Arithmetic and comparisons on a numeric alias (`---@alias Size 100 | 80 | 60`) are no longer reported as `invalid-op`, and unary minus works on an opaque numeric alias.
- `tContains`, `Clamp`, `TableIsEmpty` and other functions that retail implements in C, but the Classic clients define in FrameXML, are no longer `wrong-flavor-api` in Classic projects.
- Editing a `@meta` declaration file that the workspace reads, in the workspace or in a `library` folder, now updates the files that use it.
- A positional table entry that starts with a name (`{ a == b, f (1) }`) is now parsed as the whole expression, so the table's element type is correct.
- An optional type that doesn't exist (`@param w MissingType?`) now types the value as `any`, so calls are no longer reported as passing a value where `nil` is expected.
- Hover now puts parentheses around a function-valued return when more returns follow it: `-> (fun(): string), table` instead of the ambiguous `fun(): string, table`.
