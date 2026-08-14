-- Cross-file `@class` field-type harvesting: definitions.
--
-- Runtime `self.x = <expr>` fields on a `@class` whose coarse cross-file scan type
-- decays to `any` are upgraded on first cross-file read to the definition-site type
-- (the `any`-only slice of the lossless-cross-file work). See analysis/deferred.rs.

local addonName, ns = ...

---@class CFT_Bar
local Bar = {}
ns.CFT_Bar = Bar
function Bar:Ping() end

---@class CFT_Reg
local Reg = {}
ns.CFT_Reg = Reg

---@type CFT_Bar
local gBar

---@type table
local gTbl

function Reg:Init()
    self.plain = gBar        -- upvalue typed CFT_Bar: coarse `any` -> CFT_Bar
    self.num = 1 + 2         -- arithmetic: coarse `any` -> number
    self.opt = gBar          -- assigned CFT_Bar here...
    self.loose = gTbl        -- RHS is a bare `table`: MUST stay coarse `any`, never
end                          -- upgrade `any`->`table` (would false-positive cannot-call
                             -- on a metatable/mixin/callable — see contains_coarse_placeholder)

function Reg:Clear()
    self.opt = nil           -- ...and nil here -> nilability carried (CFT_Bar?)
end
