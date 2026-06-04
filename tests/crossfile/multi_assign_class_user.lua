-- Cross-file test: methods defined on multi-assignment @class are visible from other files.

---@type MultiAssignLib
local Lib = GetLib()

-- Cross-file methods should be accessible (not undefined-field)
Lib:Release("test")
--  ^ hover: (method) function MultiAssignLib:Release(item: string)  def: external

Lib:GetName()
--  ^ hover: (method) function MultiAssignLib:GetName()  def: external

-- @field from the lib file should work
local v = Lib.version
--    ^ hover: (local) v: number  def: local
