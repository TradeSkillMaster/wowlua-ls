-- Cross-file frame-factory return test: a plain factory in its OWN file.
-- It injects no fields, and (crucially) shares no file with a field-injecting
-- factory — `overlay_fields` is per-file, so this factory's `Frame` return must
-- stay the precise bare `Frame` class with no inline shape attached.
-- Requires: --with-stubs

local addonName, ns = ...

ns.PlainComponents = {}

function ns.PlainComponents.GetPlainFrame(parent)
    return CreateFrame("Frame", nil, parent)
end
