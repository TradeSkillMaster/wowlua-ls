---@diagnostic disable: unused-local, empty-block
-- Regression: a module table assigned onto the addon namespace. The coarse
-- cross-file workspace scan registers only the methods defined as
-- `function Manager:M` (it does not descend into bodies), while the per-file
-- engine additionally resolves the `self.field` assignments inside those
-- bodies. The namespace field's type must therefore be the single precise
-- table (methods + self-fields), NOT a spurious `{... N fields} | {... M fields}`
-- union of one logical table (the body-blind scan view unioned with the
-- precise engine view).
--- @class ModShapeNS
local _, ns = ...

local Manager = {}
ns.Manager = Manager

function Manager:Init()
    self.frame = nil
    self.active = false
end

function Manager:Start() end

function Manager:Stop() end

local mgr = ns.Manager
--    ^ hover: (local) mgr: {\n  Init: fun(self: table),\n  Start: fun(self: table),\n  Stop: fun(self: table),\n  active: false,\n  frame: nil\n}

-- Verify that genuinely different tables in a union receiver are NOT collapsed,
-- even when one's field set is a subset of another's.
---@class ShapeA
---@field x number
---@field y number

---@class ShapeB
---@field x number
---@field y number
---@field z number

---@type ShapeA | ShapeB
local ab

local abx = ab.x
--    ^ hover: (local) abx: number

---@class ShapeC
---@field a number
---@field b number
---@field c number

---@class ShapeD
---@field b number
---@field c number
---@field d number

---@type ShapeC | ShapeD
local cd

local cdb = cd.b
--    ^ hover: (local) cdb: number
