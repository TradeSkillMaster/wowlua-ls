-- Classes the base class's constructor fields are instances of. The base
-- class's coarse scan can't type `self._header = header` (a local) nor
-- `self._bindings = MakeBindings()` (a cross-file call), so both are `any`
-- placeholders until the per-file harvest.
---@class Widget
---@field name string
local Widget = {}

---@return Widget
function MakeWidget()
    return Widget
end

---@class Bindings
local Bindings = {}

---@param key string
function Bindings:Add(key) end

---@return Bindings
function MakeBindings()
    return Bindings
end
