-- A local table exported to two fields is one table: a function called through
-- one of them is used, and one never called is reported once, not taken for a
-- method two tables define.
local _, ns = ...

local Exported = {}
function Exported.Init() return 1 end
function Exported:NeverCalled() end
ns.Exported = Exported
ns.registry = { exported = Exported }
