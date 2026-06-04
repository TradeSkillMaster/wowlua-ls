-- Cross-file test: re-declaring @class with @field must still see cross-file methods.
-- Regression: the has_source_fields filter was skipping method imports.

---@class FieldMethodLib
---@field enabled boolean
local Lib = {}

-- Cross-file methods should be accessible (not undefined-field)
Lib:ReleaseItem("test")
--  ^ hover: (method) function FieldMethodLib:ReleaseItem(tooltip: string)  def: external

Lib:GetName()
--  ^ hover: (method) function FieldMethodLib:GetName()  def: external

-- Cross-file dot-style function field should also be accessible
Lib.IsValid("x")
--  ^ hover: (field) function FieldMethodLib.IsValid(key: string)  def: external

-- @field from this file should work
local e = Lib.enabled
--    ^ hover: (local) e: boolean  def: local

-- @field from the other file should work
local v = Lib.version
--    ^ hover: (local) v: number  def: local
