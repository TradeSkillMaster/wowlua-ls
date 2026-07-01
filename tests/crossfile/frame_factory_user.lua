---@diagnostic disable: unused-local
-- Cross-file frame-factory return test: usage
-- The injected fields (`DropDown`, `Label`) must NOT report `undefined-field`,
-- they must hover/complete with their precise types, and the real `Frame`
-- methods must still resolve on the returned instance. (The exhaustive harness
-- also fails on any unexpected diagnostic, so the absence of `undefined-field`
-- on the injected-field accesses below is itself an assertion.)

local addonName, ns = ...

local dropdown = ns.Components.GetBasicDropdown(nil)
--    ^ hover: (local) dropdown: Frame & {... 4 fields} {
--            ^ hint: : Frame & {... 4 fields}
-- A large injected shape caps to a field count in BOTH hover and hint (the
-- existing anonymous-table `{... N fields}` convention); each field still
-- hovers/navigates on its own access below.

-- Injected per-instance fields carried cross-file as an inline table shape:
-- precise hover, no false `undefined-field`, the field's own methods resolve,
-- and go-to-definition jumps to the field's assignment in the factory's file.
dropdown.DropDown:SetWidth(250)
--       ^ hover: (field) DropDown: DropdownButton & WowStyle1DropdownTemplate  def: external
dropdown.Label:SetText("hi")
--       ^ hover: (field) Label: FontString
-- Method-style injected field, including go-to-def on the method *call* (the
-- reported case): `:SetValue()` resolves cross-file to the factory definition.
dropdown:SetValue(1)
--       ^ def: external

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

-- A factory injecting a single field: the narrowed shape is small enough to
-- render inline in the inlay hint (no field-count cap), which is the concise,
-- precise form for a per-instance factory result.
local toggle = ns.Components.GetToggle(nil)
--    ^ hover: (local) toggle: Frame & { Toggle: fun(self: Frame) } {
--          ^ hint: : Frame & { Toggle: fun(self: Frame) }
toggle:Toggle()

-- Field injected via a local alias of the returned frame (`local f2 = frame`):
-- the shape still carries it, so it resolves cross-file with no false
-- `undefined-field`, and go-to-def jumps to the aliased write.
local aliased = ns.Components.GetAliased(nil)
--    ^ hover: (local) aliased: Frame & { Aliased: Frame } {
aliased.Aliased:SetShown(true)
--      ^ hover: (field) Aliased: Frame  def: external
