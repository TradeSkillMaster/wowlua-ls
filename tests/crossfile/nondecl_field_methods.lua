-- Defines methods on `ns.NDF_Owner` WITHOUT a local `@class NDF_Owner` declaration, so
-- `self` resolves to the *external* class table. These `self.x = ...` writes reach the
-- harvest only through external-table matching over the class's assigning-file index
-- (this file is indexed for `NDF_Owner` because it assigns `NDF_Owner`'s fields, even
-- though it does not declare the class). Also declares a co-located sibling class to
-- exercise whole-file warming through that same external assigning-file re-analysis.

local addonName, ns = ...

---@type NDF_Widget
local gWidget

function ns.NDF_Owner:Build()
    self.widget = gWidget    -- upvalue typed NDF_Widget: coarse `any` -> NDF_Widget
    self.count = 10 + 5      -- arithmetic: coarse `any` -> number
    self.opt = gWidget       -- assigned NDF_Widget here...
end

function ns.NDF_Owner:Reset()
    self.opt = nil           -- ...and nil here -> nilability carried (NDF_Widget?)
end

-- A co-located sibling class declared (and its field assigned) in this same non-declaring
-- method file: harvested via whole-file warming when `NDF_Owner` is read, since this
-- file is re-analyzed for `NDF_Owner` and its whole decl-set {this file} is covered.
---@class NDF_Helper
local Helper = {}
ns.NDF_Helper = Helper
function Helper:Init()
    self.tag = 3 + 4         -- coarse `any` -> number
end
