-- A class declared on a namespace table another file creates, so its own
-- methods read `self` fields through the cross-file class: the harvest of
-- GetOffset's return reads the `offset` field harvest of this same file.
local addonName, ns = ...

---@class OrderWidget
ns.UI.Widget = {}

function ns.UI.Widget:Resize(height, scale)
    self.offset = -height / 2 * scale
end

function ns.UI.Widget:GetOffset()
    return self.offset
end
