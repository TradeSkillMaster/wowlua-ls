---@diagnostic disable: missing-return, unused-local
-- A defclass-created class whose constructor self-field `_m` is a chained generic
-- call. The coarse scan types `_m` as `any`; the harvest recovers HarvestMgr<HarvestState>.

---@class HarvestState : HarvestBase
---@field flag boolean
---@field level number

---@return HarvestState
local function makeState() end

local Holder = HarvestObj.DefineClass("Holder")

function Holder:__init()
    local state = makeState()
    self._m = HarvestMgr.Create(state)
        :Suppress("a")
        :Suppress("b")
end

function Holder:Use()
    -- Field's generic type args harvested: T = HarvestState
    self._m:SetFromExpr("x", [[flag and level > 0]])
--       ^ hover: (field) _m: HarvestMgr<HarvestState>
--                             ^ hover: (field) flag: boolean

    -- Builtins intersection member is still available
    self._m:SetFromExpr("y", [[rand() > 0.5]])
--                             ^ hover: (field) rand: fun(): number

    -- A genuinely-undefined name is flagged against the harvested context
    self._m:SetFromExpr("z", [[missingField]])
--                             ^ diag: undefined-field
end
