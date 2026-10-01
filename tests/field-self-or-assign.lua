---@diagnostic disable: unused-local
-- A field seeded `nil` in a module table and later written through its own
-- value (`t.f = t.f or Create()`) resolves to the callee's return type: the
-- field's in-progress write is a cycle, not an unresolvable value that widens
-- the field to `any`. Covers a field that is read back and one nothing reads
-- (only the fixpoint resolves that write — the query-time resolver cannot
-- break its self-cycle), across the cache clear a later backward-inferred
-- parameter triggers.

---@class Query
---@field Run fun(self: Query): number
local Query = {}

---@return Query
local function CreateQuery()
    return Query
end

local private = {
    read = nil,
    unread = nil,
}

function private.GetRead()
    private.read = private.read or CreateQuery()
    --      ^ hover: (field) read: Query
    return private.read
end

function private.Init()
    private.unread = private.unread or CreateQuery()
    --      ^ hover: (field) unread: Query
end

-- An unannotated parameter inferred from its use makes the fixpoint's fallback
-- branch clear the expression cache; the field writes are re-seeded after it.
function private.Bump(n)
    return n + 1
end

local q = private.GetRead()
--    ^ hover: (local) q: Query

_G.useSelfOrAssign = { private, q }
