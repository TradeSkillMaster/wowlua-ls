### Bug Fixes

- Fixed false `undefined-field` errors on a local assigned from a chained generic-getter call. When the chain's outer method took a class-naming string argument (e.g. `getReg():asType("Wrapped")`), the variable was mis-typed as that named class instead of the method's real return type — so legitimate fields on the actual type, including a field the value was later stored into, were flagged as undefined.
- Fixed a false `type-mismatch` on `Addon:NewModule("Name", "AceEvent-3.0")`. AceAddon's `NewModule` accepts either a prototype table or an Ace library name string as its second argument, but the parameter was typed `table` only.
