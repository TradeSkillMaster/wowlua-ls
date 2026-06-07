---@diagnostic disable: unused-local
-- Cross-file caller: the inferred return type of `GetItem` should resolve to the
-- precise class type via lazy whole-file analysis, not the coarse `any`.

---@class InfRepo
local InfRepo = {}

local item = InfRepo:GetItem("x")
--    ^ hover: (local) item: InfThing?  def: local
--                   ^ hover: (method) function InfRepo:GetItem(key)\n-> InfThing?  def: external
