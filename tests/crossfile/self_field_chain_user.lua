---@diagnostic disable: unused-local
-- Consumer re-declares @class ChainHost (as every addon module re-declares its
-- addon-namespace @class). The chained self-field `handle` from the lib must be
-- visible here with no `undefined-field` — exercising that the existence-only
-- `table` registration survives the prescan overlay-import filter (which drops
-- bare unannotated table placeholders, but not annotation-carrying fields).

---@class ChainHost
local H = {}

function H:Use()
    local h = self.handle
    --              ^ hover: (field) handle: table
    return h
end
