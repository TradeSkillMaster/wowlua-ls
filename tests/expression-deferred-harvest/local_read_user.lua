---@diagnostic disable: missing-return, unused-local
-- A module-private-table field holding a manager whose generic comes from a
-- builder-built state, read into a local in a function above the setup: the
-- local doesn't keep the base binding an early pass gave the field read.

local private = {
    manager = nil,
}

local STATE_SCHEMA = HarvestSchema.Create("LocalReadState")
    :AddBool("flag")
    :Commit()

-- `flag` is a field of the built state, so the expression reports nothing here
-- (the harness fails on any diagnostic left unasserted).
function private.Use()
    local m = private.manager
    m:SetFromExpr("x", [[flag and rand() > 0]])
end

function private.Setup()
    local state = STATE_SCHEMA:CreateState()
    --    ^ hover: (local) state: LocalReadState
    private.manager = HarvestMgr.Create(state)
end

---@class LocalReadState : HarvestBase
