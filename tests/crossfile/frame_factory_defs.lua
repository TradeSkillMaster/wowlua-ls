-- Cross-file frame-factory return test: definitions
-- A factory function that creates a Frame and injects extra fields on the
-- instance, then returns it. The injected fields (`DropDown`, `Label`) are a
-- per-file overlay on the external `Frame` class, so the cross-file lift carries
-- them on the returned instance (`Frame & { DropDown: … }`), where they hover,
-- complete and go to their definition.
-- Requires: --with-stubs

local addonName, ns = ...

ns.Components = {}

function ns.Components.GetBasicDropdown(parent)
    local frame = CreateFrame("Frame", nil, parent)
    local dropdown = CreateFrame("DropdownButton", nil, frame, "WowStyle1DropdownTemplate")
    local label = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
    frame.Init = function(_, entries) end
    frame.Label = label
    frame.DropDown = dropdown
    -- Method-style injected field: exercises the `function recv:Method()` path
    -- (distinct from the `recv.field = ...` assignment path above) for both the
    -- per-instance shape narrowing and go-to-definition.
    function frame:SetValue(value) end
    return frame
end

-- A factory injecting a single field: its narrowed shape stays small enough to
-- render inline in an inlay hint (`Frame & { Toggle: fun… }`), rather than the
-- field-count cap a larger shape gets.
function ns.Components.GetToggle(parent)
    local frame = CreateFrame("Frame", nil, parent)
    function frame:Toggle() end
    return frame
end

-- Field injected with an anonymous *record* value (`frame.Config = { ... }`):
-- before the record lift these decayed to `any` and were dropped from the
-- overlay entirely (the `contains_any` filter in `wrap_overlay_shape`); now the
-- overlay carries the field with its record shape cross-file. Exercises the
-- overlay lift site's `lift_local_type_to_ext_with` parity with the return path.
function ns.Components.GetConfigured(parent)
    local frame = CreateFrame("Frame", nil, parent)
    frame.Config = { width = 10, height = 20 }
    return frame
end

-- Field injected through a *local alias* of the returned frame: the write's
-- receiver is `f2`, not `frame`, but it lands on the returned instance. Its
-- shape must still carry `Aliased` (per-instance narrowing resolves the alias),
-- else the field falsely reports `undefined-field` at the cross-file call site.
function ns.Components.GetAliased(parent)
    local frame = CreateFrame("Frame", nil, parent)
    local f2 = frame
    f2.Aliased = CreateFrame("Frame", nil, parent)
    return frame
end

-- A frame created from a template carries what is written through it too.
function ns.Components.GetTemplated(parent)
    local frame = CreateFrame("Frame", nil, parent, "BackdropTemplate")
    frame.Caption = "Title"
    function frame:Refresh() end
    return frame
end

-- So does one a local helper of this file builds.
local function BuildDialog(parent)
    local dialog = CreateFrame("Frame", nil, parent, "BackdropTemplate")
    function dialog:Update() end
    return dialog
end

function ns.Components.GetDialog(parent)
    local dialog = BuildDialog(parent)
    return dialog
end

-- A named frame's global is the frame the local its call initializes holds: with
-- what is written through the local, or the class the local is declared.
local named = CreateFrame("Frame", "FactoryNamedFrame", UIParent, "BackdropTemplate")
named.state = 1
function named:Open() end

---@class FactoryOptionsFrame : Frame
---@field title string
local options = CreateFrame("Frame", "FactoryOptionsGlobal", UIParent, "BackdropTemplate")
function options:Refresh() end
