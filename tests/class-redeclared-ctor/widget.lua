-- An AceGUI-style widget: the class types its frames as intersections with an
-- `{obj: self}` shape, its constructor redeclares it bare, and each frame gets
-- its `obj` once the widget table exists.

local TYPE = "RedeclButton"

--- @class RedeclButton : AceGUIWidget
--- @field public frame Frame&{obj: self}
--- @field protected background Texture&{obj: self}
--- @field protected mask MaskTexture&{obj: self}?
--- @field protected visible boolean
local RedeclButton = {}

function RedeclButton:Show()
    self.visible = true
end

local function Constructor()
    local frame = CreateFrame("Button", nil, UIParent)
    local background = frame:CreateTexture(nil, "BACKGROUND")
    local mask = frame:CreateMaskTexture()
    --- @class RedeclButton
    local widget = {
        type = TYPE,
        frame = frame,
        background = background,
        mask = mask,
--      ^ diag: field-type-mismatch
        visible = false,
    }
    frame.obj = widget
    background.obj = widget
    return widget
end

-- A second widget in the file: its frames take its own `obj` writes, not the
-- first widget's, and a write of the wrong value doesn't count.

--- @class RedeclToggle : AceGUIWidget
--- @field public frame Frame&{obj: self}
--- @field protected check Texture&{obj: self}
local RedeclToggle = {}

function RedeclToggle:Show()
    self.frame:Show()
end

local function ToggleConstructor()
    local frame = CreateFrame("Button", nil, UIParent)
    local check = frame:CreateTexture(nil, "OVERLAY")
    --- @class RedeclToggle
    local widget = {
        type = "RedeclToggle",
        frame = frame,
        check = check,
--      ^ diag: field-type-mismatch
    }
    frame.obj = widget
    check.obj = frame
    return widget
end

return Constructor, ToggleConstructor
