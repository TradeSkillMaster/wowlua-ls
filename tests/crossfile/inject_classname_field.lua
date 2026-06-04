-- Regression test: inject-field false positive when field name coincides with
-- a WoW class name (e.g. "Background" matches the Background FrameXML class).
-- The workspace scan's second pass in build_on_stubs resolves Unknown fields by
-- matching field names against class names, creating Table(Some(idx)) entries
-- that bypass the prescan placeholder filter and set field_existed_at_build=true.
-- All four field assignments below should behave consistently (no inject-field).

---@class InjectClassNameTest : Frame
local Widget = CreateFrame("Frame", "InjectClassNameTestFrame", UIParent)

local pulse = Widget:CreateTexture(nil, "BACKGROUND")
--    ^ hover: (local) pulse: Texture {
Widget.Pulse = pulse

local bg = Widget:CreateTexture(nil, "BACKGROUND")
Widget.Background = bg

local flash = Widget:CreateTexture(nil, "OVERLAY")
Widget.Flash = flash

local cd = CreateFrame("Cooldown", nil, Widget, "CooldownFrameTemplate")
Widget.secondWindCharge = cd

-- @class-annotated variables are class definitions — inject-field does not fire
-- even when @field annotations exist (the variable IS the class, not an instance).
---@class InjectClassNameContract : Frame
---@field Overlay Texture
local Annotated = CreateFrame("Frame", "InjectClassNameAnnotated", UIParent)

local tex = Annotated:CreateTexture(nil, "OVERLAY")
Annotated.Overlay = tex

Annotated.Background = tex
