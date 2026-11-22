---@diagnostic disable: unused-local, create-global

-- A member this file adds to a class another file declares lists after the
-- declaring file's members, before a third file's.
function XfOrder:Local() end

---@type XfOrder
local order = XfOrder
--    ^ hover: (local) order: XfOrder {\n  name: string,\n  count: number,\n  active: boolean,\n  Run: fun(self: XfOrder),\n  Local: fun(self: XfOrder),\n  Zeta: fun(self: XfOrder),\n  Extra: fun(self: XfOrder),\n  zulu: number,\n  alpha: string\n}

-- A class declared here lists this file's fields first, then the members another
-- file adds.
---@class XfOrderHere
---@field second number
---@field first string
XfOrderHere = {}

function XfOrderHere:Own() end

---@type XfOrderHere
local here = XfOrderHere
--    ^ hover: (local) here: XfOrderHere {\n  second: number,\n  first: string,\n  Own: fun(self: XfOrderHere),\n  Remote: fun(self: XfOrderHere)\n}

-- A parent's field this file assigns shows as this file types it, read through a
-- subclass or an intersection naming the parent first, as reading it does.
function XfOrderIA.handler(a, b) return a, b end

---@type XfOrderIB
local sub = XfOrderIB
--    ^ hover: (local) sub: XfOrderIB {\n  handler: fun(a, b): ?, ?\n}

---@type XfOrderIA & XfOrderIB
local both = nil
--    ^ hover: (local) both: XfOrderIB {\n  handler: fun(a, b): ?, ?\n}
