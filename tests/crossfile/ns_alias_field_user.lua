---@diagnostic disable: unused-local
-- Regression: assigning a field through a namespace-aliased local, then reading
-- that field back into a local, must preserve the field's type. The write
-- (`AliasFieldEnum.Thing = {...}`) is deferred past the resolution fixpoint
-- because the alias root only resolves to a writable table afterward; the
-- read-back local used to keep a stale unknown type.
local _, ns = ...

local AliasFieldEnum = ns.AliasFieldEnum

AliasFieldEnum.Thing = { a = 1, b = 2 }

-- Read-back local whose name collides with the field name.
local Thing = AliasFieldEnum.Thing
--    ^ hover: (local) Thing: {  def: local

-- Read-back local with a non-colliding name resolves identically.
local other = AliasFieldEnum.Thing
--    ^ hover: (local) other: {  def: local

-- Field access on the read-back local still reaches the inner fields.
local widthVal = Thing.a
--    ^ hover: (local) widthVal: number  def: local
