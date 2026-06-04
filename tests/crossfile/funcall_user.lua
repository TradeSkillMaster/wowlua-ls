---@diagnostic disable: unused-local
-- Cross-file funcall test: assigns function call return value to addon field
local addonName, ns = ...
ns.Comp = ns.Factory:NewComponent()

local c = ns.Comp
--    ^ hover: (local) c: MyComponent {  def: local
local n = ns.Comp.name
--    ^ hover: (local) n: string  def: local
local a = ns.Comp.active
--    ^ hover: (local) a: boolean  def: local
