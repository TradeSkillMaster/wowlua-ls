---@diagnostic disable: unused-local
-- Consumer re-declares @class ChainHost (as every addon module re-declares its
-- addon-namespace @class). The chained self-field `handle` from the lib must be
-- visible here with no `undefined-field` — exercising that the existence-only
-- `any` registration survives the prescan overlay-import filter (which drops
-- bare unannotated table placeholders, but not annotation-carrying fields).
--
-- The coarse scan can't resolve the chain's return type, so the field is
-- registered existence-only as `any` (the honest "unknown"), NOT a guessed
-- `table` — a concrete `table` would false-positive when the chain actually
-- returns a non-table (e.g. `f():GetHeight()` -> number passed to a number
-- parameter reads as `type-mismatch`).

---@class ChainHost
local H = {}

function H:Use()
    local h = self.handle
    --              ^ hover: (field) handle: any
    return h
end
