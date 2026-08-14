---@diagnostic disable: unused-local
-- Cross-file `@class` field-type harvesting: usage.
-- Runtime self-fields whose coarse cross-file type is `any` are upgraded to the
-- definition-site type on read. See class_field_type_defs.lua.

local addonName, ns = ...

---@type CFT_Reg
local r = ns.CFT_Reg

-- A class-instance field: coarse `any` upgraded to its class (RHS-aware harvest).
local p = r.plain
--    ^ hover: (local) p: CFT_Bar
-- Because the field is now precisely `CFT_Bar`, a real method resolves — and a
-- bogus one would fire `undefined-field` (caught by the exhaustive harness).
r.plain:Ping()

-- A primitive field: coarse `any` upgraded to `number`.
local n = r.num
--    ^ hover: (local) n: number

-- A field assigned a class in one method and `nil` in another: the harvested type
-- carries the field's nilability (`CFT_Bar?`), never sharpening to a non-optional
-- `CFT_Bar` that would false-positive when a caller clears it.
local o = r.opt
--    ^ hover: (local) o: CFT_Bar?

-- A field whose RHS resolves to a bare `table` stays coarse `any` — never upgraded
-- `any`->`table`, which is *more* restrictive than `any`. The call below must NOT
-- fire `cannot-call` (a bare `table` isn't callable, but `any` is); the exhaustive
-- harness fails on any spurious diagnostic, so its absence is the assertion.
local l = r.loose
--    ^ hover: (local) l: any
r.loose()
