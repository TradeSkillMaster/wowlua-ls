---@diagnostic disable: missing-return, unused-local
-- Module-private-table field (NOT a defclass self-field) assigned a manager whose
-- generic comes from a builder-built state. The state's type refines from the base
-- `HarvestBase` to the built subtype `ModState` late in the fixpoint; the expression
-- context must use the refined subtype, not a cached stale base binding. (Regression
-- for field_type_args_cache poisoning of call_type_args-derived type args.)

local private = {
    manager = nil,
}

local STATE_SCHEMA = HarvestSchema.Create("ModState")
    :AddBool("flag")
    :Commit()

function private.Setup()
    local state = STATE_SCHEMA:CreateState()
    private.manager = HarvestMgr.Create(state)

    -- `flag` exists only on the built subtype ModState, not on the base HarvestBase.
    -- If T were cached as the base, this would falsely report `flag` undefined.
    private.manager:SetFromExpr("x", [[flag and rand() > 0]])
--                                     ^ hover: (field) flag: boolean

    -- A genuinely-undefined name is still flagged against the refined context
    private.manager:SetFromExpr("z", [[nope]])
--                                     ^ diag: undefined-field
end

-- The built state is the manager's type argument
---@class ModState : HarvestBase
