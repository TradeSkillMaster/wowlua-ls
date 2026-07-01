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
