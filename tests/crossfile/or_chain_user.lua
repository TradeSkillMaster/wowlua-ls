-- Cross-file test: or-chaining on addon namespace fields (usage)
local _, ns = ...

-- `ns.CompareByName` should resolve as function, not table.
-- Passing it to table.sort should not produce a type-mismatch diagnostic.
local items = {3, 1, 2}
table.sort(items, ns.CompareByName)
--                   ^ hover: (field) CompareByName: function  def: external  diag: none
