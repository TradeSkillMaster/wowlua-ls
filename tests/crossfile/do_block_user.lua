---@diagnostic disable: unused-local
-- Cross-file test: methods defined inside do...end blocks should be visible
local addonName, ns = ...
local DBC = ns.DBC

local a = DBC:TopLevel()
--    ^ hover: (local) a: string

local b = DBC:InsideDo()
--    ^ hover: (local) b: number

local c = DBC:NestedDo()
--    ^ hover: (local) c: boolean

local d = DBC.StaticField
--    ^ hover: (local) d: string
