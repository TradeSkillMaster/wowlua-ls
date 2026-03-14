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
