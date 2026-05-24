-- Cross-file test: Mixin() with addon namespace @class field preserves class name
local _, ns = ...

local frame = CreateFrame("Frame")
Mixin(frame, ns.UI.AlphaMixin)
-- After @narrows-arg, frame should be Frame & NsMixinAlpha (not anonymous table)
local _ = frame
--    ^ hover: (local) _: Frame & NsMixinAlpha

-- Self-scanned methods should be accessible through the class type
frame:OnLoad()
--     ^ hover: (method) function NsMixinAlpha:OnLoad()  def: external
frame:GetMixinLabel()
--     ^ hover: (method) function NsMixinAlpha:GetMixinLabel()\n  -> string  def: external

-- @field declaration on @class takes precedence over self-scanned field assignment
local p = ns.UI.AlphaMixin.priority
--    ^ hover: (local) p: number  def: local
