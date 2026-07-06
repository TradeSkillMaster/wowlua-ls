-- Indirect form of the chained generic-getter false positive: a local assigned
-- from a chained call whose *transforming* outer colon method takes a class-naming
-- string first arg must not make the local (nor a field it is stored into) that
-- named class. `getReg():asType("Wrapped")` returns `Reg`, not `Wrapped` — so the
-- coarse defclass-local heuristic (`local X = Base:Init("Class")`) must skip it.
---@diagnostic disable: unused-local, missing-return

---@class Wrapped
---@field only_on_wrapped fun()
local Wrapped = {}

---@class Reg
local Reg = {}
---@param className string
---@return Reg
function Reg:asType(className) end
---@return number
function Reg:val() end

---@return Reg
local function getReg() end

---@class IndirectNS
local IndirectNS = {}

-- Outer `:asType` takes class-naming string "Wrapped" but returns Reg.
local r = getReg():asType("Wrapped")
IndirectNS.stored = r
local v = IndirectNS.stored:val()
--    ^ hover: (local) v: number
