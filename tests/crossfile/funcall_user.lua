-- Cross-file funcall test: assigns function call return value to addon field
local addonName, ns = ...
ns.Comp = ns.Factory:NewComponent()

local c = ns.Comp
--    ^ hover: c: MyComponent  def: local
local n = ns.Comp.name
--    ^ hover: n: string  def: local
local a = ns.Comp.active
--    ^ hover: a: boolean  def: local
