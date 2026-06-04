---@diagnostic disable: unused-local
-- Cross-file defclass overlay test: child class with @class overlay that overrides a field.
-- Regression test: adding @field via @class overlay must NOT lose __super from defclass inheritance.

---@class OverlayExtraInfo
---@field detail string

-- Standalone @class overlay that adds a field to a defclass-discovered class.
-- This must NOT shadow the defclass entry or lose __super.
---@class OverlayChild
---@field extraInfo OverlayExtraInfo

local OverlayParent = DefineClassWithParent("OverlayParent")
local OverlayChild = DefineClassWithParent("OverlayChild", OverlayParent)

-- The overlay field should be accessible
local info = OverlayChild.extraInfo
--    ^ hover: (local) info: OverlayExtraInfo {

-- __super should still be typed as OverlayParent (not lost due to overlay)
local sup = OverlayChild.__super
--    ^ hover: (local) sup: OverlayParent

-- Inherited baseMethod should work directly on the child too
OverlayChild:baseMethod()

-- __super calls inside methods should not produce warnings
function OverlayChild:DoStuff()
    self.__super:baseMethod()
end
