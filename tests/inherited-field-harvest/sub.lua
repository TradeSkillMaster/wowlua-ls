---@diagnostic disable: unused-local
-- A subclass in another file reads fields its base class declares only by
-- assignment. The coarse `any` placeholders are harvested from the base's
-- declaring file even though the receiver (`self`) is this file's own class
-- table: the overlay is keyed by the declaring ancestor, and the subclass's
-- prescan copy of the inherited field is refreshed from it. Sub-fields the
-- base wrote onto `_header` (via the chain and via the local alias) resolve
-- too instead of tripping `undefined-field`.
---@class Sub: Base
local Sub = {}

function Sub:Use()
    local h = self._header
    --    ^ hover: (local) h: Widget & { cells: table, count: number, extra: string }
    local c = self._header.cells
    --    ^ hover: (local) c: table
    local n = self._header.count
    --    ^ hover: (local) n: number
    local e = self._header.extra
    --    ^ hover: (local) e: string
    local name = self._header.name
    --    ^ hover: (local) name: string
    self._bindings:Add("k")
    --             ^ hover: (method) function Bindings:Add(key: string)
end

---@param b Base
local function useBase(b)
    local n = b._header.count
    --    ^ hover: (local) n: number
end

_G.useInheritedFieldHarvest = { Sub, useBase }
