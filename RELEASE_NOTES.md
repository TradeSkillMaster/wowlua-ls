**New**

- `invalid-op` diagnostic: warns when arithmetic operators are used on strings (suggests `..` for concatenation), arithmetic on booleans/nil/tables, or concatenation on non-stringable types ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/diagnostics.html#invalid-op))

**Bug Fixes**

- Fixed false positive `cannot-call` on fields whose assignments couldn't be fully resolved
