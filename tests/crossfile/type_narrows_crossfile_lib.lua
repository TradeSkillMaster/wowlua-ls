local _, ns = ...

---@class TNBase
local TNBase = {}
ns.TNBase = TNBase

---@generic C
---@param class C
---@type-narrows 0 1
---@return boolean
---@diagnostic disable-next-line: missing-return
function TNBase:__isa(class) end

---@class TNChild : TNBase
---@field extra string
local TNChild = {}
ns.TNChild = TNChild

---@class TNCreature
---@field name string
local TNCreature = {}
ns.TNCreature = TNCreature

---@class TNFeline : TNCreature
---@field purrs boolean
local TNFeline = {}
ns.TNFeline = TNFeline

---@type-narrows TNFeline
---@return boolean
---@diagnostic disable-next-line: missing-return
function TNCreature:IsFeline() end

---@param task TNBase
function ns.DiscardTask(task) end

---@return fun(): number, TNBase
---@diagnostic disable-next-line: missing-return
function ns.TaskIterator() end

---@return fun(): number, TNCreature
---@diagnostic disable-next-line: missing-return
function ns.CreatureIterator() end
