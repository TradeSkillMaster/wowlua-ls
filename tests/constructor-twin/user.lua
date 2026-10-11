---@diagnostic disable: unused-local
-- Another file reads the constructor through the workspace scan: a call-valued
-- entry, or one naming a local assigned a call, is typed from the callee's
-- return once every global is registered, and an inline `--[[@as T]]` cast on
-- an entry is honored.
local a = NS.CONST.A
--    ^ hover: (local) a: Widget
local b = NS.CONST.B
--    ^ hover: (local) b: Widget
local f = NS.CONST.F
--    ^ hover: (local) f: Widget
local c = NS.CONST.C
--    ^ hover: (local) c: number
local e = NS.CONST.E
--    ^ hover: (local) e: Special


---@param cfg Config
local function readConfig(cfg)
    local h = cfg.handler
    --    ^ hover: (local) h: any
    return h
end
local declared = NS.DECLARED.h
--    ^ hover: (local) declared: any
local declaredAbove = NS.DECLARED.w
--    ^ hover: (local) declaredAbove: Widget
local replaced = NS.REPLACED.A
--    ^ hover: (local) replaced: Other
local gReplaced = G_REPLACED.A
--    ^ hover: (local) gReplaced: Other
local shadowed = NS.SHADOWED.A
--    ^ hover: (local) shadowed: any
local gA = G_ENTRIES.A
--    ^ hover: (local) gA: Widget
local gB = G_ENTRIES.sub.B
--    ^ hover: (local) gB: Widget

_G.useConstructorEntries = { a, b, c, e, f, readConfig, declared, declaredAbove, replaced, gReplaced, shadowed, gA, gB }
