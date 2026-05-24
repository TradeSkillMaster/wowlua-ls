-- Cross-file generic funcall test: generic function whose return type
-- cannot be resolved during cross-file scanning, causing a placeholder
-- table annotation on the namespace field.

---@class MixinA
---@field greet fun(self): string
local MixinA = {}

---@generic T
---@param mixin T
---@return T
function MakeInstance(mixin) return {} end

local addonName, ns = ...
ns.MixinA = MixinA
