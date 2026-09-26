---@meta _

---@class GameTooltip
local GameTooltip = {}

-- `owner` is intentionally untyped: it may be any frame or WORLDFRAME, and annotating
-- it as Frame causes false positives when addon mixins pass `self`.
---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetOwner)
---@param owner any The frame or WorldFrame that owns the tooltip
---@param anchor TooltipAnchor
---@param x? number
---@param y? number
function GameTooltip:SetOwner(owner, anchor, x, y) end
