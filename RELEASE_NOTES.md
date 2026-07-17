### Bug Fixes

- Indexing into an undefined global (e.g. `Undefined[2] = ...`) is now flagged. Previously the bracket-write registered the base name as a defined global, silently masking the `undefined-global` warning on the read that would error at runtime if it's `nil`.
- Type completion now works inside generic type annotations (e.g. the type arguments of `Foo<…>`), where it was previously missing.
- Parameter-name inlay hints are no longer shown on trailing arguments that spread into named parameters, where the hint was misleading.
- Long literal-union inlay hints are now shortened so adjacent hints on the same line are no longer dropped.
