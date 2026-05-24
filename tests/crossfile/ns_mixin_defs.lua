-- Cross-file test: defines @class on deep addon namespace field with methods
local _, ns = ...
ns.UI = ns.UI or {}

---@class NsMixinAlpha
---@field priority number
ns.UI.AlphaMixin = {}

function ns.UI.AlphaMixin:OnLoad()
end

function ns.UI.AlphaMixin:GetMixinLabel()
    return "alpha"
end

-- Self-scanned field assignment; @field declaration on @class should win
ns.UI.AlphaMixin.priority = "low"
