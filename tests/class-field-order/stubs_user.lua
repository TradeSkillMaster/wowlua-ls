---@diagnostic disable: unused-local, inject-field

-- A frame of a described class given a member of its own lists the class's
-- fields with their descriptions (`class_field_order_stub_parents`).
local holder = {}
holder.panel = CreateFrame("xforderpanel", nil, nil, "BackdropTemplate")
holder.panel.owner = holder
local panel = holder.panel

-- A field this file writes on one frame isn't a member of the other frame
-- classes (`class_field_order_stub_parents`).
local plain = CreateFrame("Frame")
plain.injectedThing = 5

---@type Button
local frameButton = nil
