-- Cross-file frame-factory return test: definitions
-- A factory function that creates a Frame and injects extra fields on the
-- instance, then returns it. The injected fields (`DropDown`, `Label`) are a
-- per-file overlay on the external `Frame` class and can't be carried across
-- files, so the cross-file lift must treat the return as a concrete instance
-- that may hold untracked runtime fields — otherwise accessing the injected
-- fields at the call site would falsely report `undefined-field`.
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
    return frame
end
