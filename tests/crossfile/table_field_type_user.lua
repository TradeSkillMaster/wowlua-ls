---@diagnostic disable: unused-local
-- Cross-file `@class` field-type harvesting: usage of the bare-`table` slice.
-- Fields whose coarse cross-file type is a bare `table` (`Table(None)` placeholder)
-- are upgraded to the definition-site type on read. See table_field_type_defs.lua.

local addonName, ns = ...

---@type TFT_Reg
local r = ns.TFT_Reg

-- A bare `table` placeholder field upgraded to its class on cross-file read.
local b = r.built
--    ^ hover: (local) b: TFT_Widget
-- Because the field is now precisely `TFT_Widget`, its method resolves — a bogus one
-- would fire `undefined-field` (caught by the exhaustive harness), and the pre-upgrade
-- bare `table` would fire `undefined-field` on this call too.
r.built:Ping()

-- A bare `table` placeholder upgraded to a primitive: the RHS-aware harvest reads the
-- real return type (`number`), not the coarse `table` the scan parked.
local n = r.num
--    ^ hover: (local) n: number
