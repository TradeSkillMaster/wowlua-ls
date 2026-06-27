-- Cross-file regression: a top-level field write whose right-hand side is a
-- *bare local variable* must register (existence-only) even when that local
-- can't be typed cross-file, so reads in other files don't false-positive as
-- `undefined-field`. Two trigger shapes:
--
--   1. `local lib = LibStub("Name-1.0"); Util.Library = lib` — the defclass-style
--      string-arg heuristic synthesizes a bogus class name "Name-1.0" from the
--      call, so the field carries a `returns` annotation that resolves to no real
--      class. (The library name is NOT a class.)
--   2. `local n = 1 + 2; Util.Computed = n` — a plain `FieldRef` to a local that
--      resolves to no global/known type.
--
-- Both used to be dropped (the coarse scan emits a `FieldRef`/bogus-`returns`
-- entry that the build couldn't resolve, and unlike the inline-literal and
-- direct-call cases there was no existence fallback). The field is registered
-- typed `any` (not bare `table`): we don't know what the local holds, and
-- committing to `table` would spuriously trip `cannot-call`/`type-mismatch` on
-- downstream uses (see bare_local_field_user.lua).
---@class BareLocalFieldNS
local ns = select(2, ...)

---@class BareLocalFieldUtil
local Util = {}
ns.Util = Util

-- (1) RHS is a local assigned from `LibStub("Name-1.0")` (the reported pattern)
local lib = LibStub("SomeMissingLibrary-1.0")
Util.Library = lib

-- (2) RHS is a plain bare local that doesn't resolve cross-file
local n = 1 + 2
Util.Computed = n
