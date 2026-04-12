-- Cross-file @built-name test: calling @built-name through wrapper functions.
-- Tests that @built-name propagates through wrapper functions for both
-- cross-file class discovery and per-file built-name resolution.
local Component = DefineClass("ChainTestComponent")
local BNReactiveSchema = Component:Include("BNReactiveSchema")
local BNReactive = Component:Include("BNReactive")

-- Call through double-wrapper (BNReactive.CreateSchema → BNReactiveSchema.Create → __init)
local STATE = BNReactive.CreateSchema("MY_BN_STATE")
    :AddStringField("label")
    :AddNumberField("count")
    :Commit()

local lbl = STATE.label
--    ^ hover: (global) lbl: string

local cnt = STATE.count
--    ^ hover: (global) cnt: number

-- Call through single-wrapper (BNReactiveSchema.Create → __init)
local STATE2 = BNReactiveSchema.Create("MY_BN_STATE2")
    :AddStringField("name")
    :Commit()

local nm = STATE2.name
--    ^ hover: (global) nm: string

-- @param referencing a @built-name class should resolve fields from the builder chain
---@param state MY_BN_STATE
function useBuiltNameParam(state)
    local sl = state.label
    --    ^ hover: (local) sl: string
    local sc = state.count
    --    ^ hover: (local) sc: number
end

-- @built-name class inherits from @return built : Parent, so no type-mismatch
---@param state BNStateBase
function acceptBaseState(state)
    local bv = state.baseVal
    --    ^ hover: (local) bv: number
end
acceptBaseState(STATE)
-- ^ diag: none

-- Generic @builds-field with backtick string literal and @param reference
local STATE3 = BNReactive.CreateSchema("MY_BN_STATE3")
    :AddOptionalClassField("item", "BNFieldBase")
    :AddStringField("name")
    :Commit()

---@param state MY_BN_STATE3
function useBuiltNameGenericParam(state)
    local si = state.item
    --    ^ hover: (local) si: BNFieldBase | nil
    local sn = state.name
    --    ^ hover: (local) sn: string
    local su = state.nonexistent
    --    ^ diag: undefined-field
end

-- Lateinit @builds-field (T!) — cross-file lateinit hover and nil assignment
local STATE_LI = BNReactive.CreateSchema("MY_BN_LI_STATE")
    :AddDeferredClassField("handler", "BNFieldBase")
    :AddStringField("tag")
    :Commit()

---@param state MY_BN_LI_STATE
function useLateinitBuiltField(state)
    state.handler:DoSomething()
    --    ^ hover: (field) handler: BNFieldBase!
    if state.handler then
        state.handler = nil
        -- ^ diag: none
    end
end

-- ── inject-field false positive on built-type field assignment ──────
-- Assigning to a built-name field should NOT fire inject-field

---@param state MY_BN_STATE
function assignBuiltField(state)
    state.label = "updated"
    -- ^ diag: none
    state.count = 99
    -- ^ diag: none
end

