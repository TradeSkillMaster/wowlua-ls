-- Cross-file test: local function assigned to addon namespace field (usage)
local private = select(2, ...)

-- `private.SortByMapName` should resolve as function, not table.
-- Passing it to table.sort should not produce a type-mismatch diagnostic.
local items = {3, 1, 2}
table.sort(items, private.SortByMapName)
--                        ^ hover: (field) SortByMapName: function  def: external  diag: none

-- Function expression assigned via local variable should also resolve as function.
private.FormatLabel("test")
--      ^ hover: (field) FormatLabel: function  def: external  diag: none
