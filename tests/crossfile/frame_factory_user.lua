---@diagnostic disable: unused-local
-- Cross-file frame-factory return test: usage
-- The injected fields (`DropDown`, `Label`) must NOT report `undefined-field`,
-- they must hover/complete with their precise types, and the real `Frame`
-- methods must still resolve on the returned instance. (The exhaustive harness
-- also fails on any unexpected diagnostic, so the absence of `undefined-field`
-- on the injected-field accesses below is itself an assertion.)

local addonName, ns = ...

local dropdown = ns.Components.GetBasicDropdown(nil)
--    ^ hover: (local) dropdown: Frame & { DropDown: DropdownButton & WowStyle1DropdownTemplate, Init: fun(_: any, entries: any), Label: FontString }

-- Injected per-instance fields carried cross-file as an inline table shape:
-- precise hover, no false `undefined-field`, and the field's own methods resolve.
dropdown.DropDown:SetWidth(250)
--       ^ hover: (field) DropDown: DropdownButton & WowStyle1DropdownTemplate
dropdown.Label:SetText("hi")
--       ^ hover: (field) Label: FontString

-- Completion surfaces the injected shape field (typed prefix filters to it).
local d = dropdown.DropDown
--                         ^ comp: DropDown

-- A real Frame method still resolves on the returned instance.
dropdown:SetPoint("TOP")

-- A plain frame factory defined in its own file (no field-injecting factory
-- shares its per-file overlay) keeps its precise bare `Frame` class.
local plain = ns.PlainComponents.GetPlainFrame(nil)
--    ^ hover: (local) plain: Frame {
plain:SetShown(true)
