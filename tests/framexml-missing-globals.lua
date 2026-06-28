---@diagnostic disable: unused-local
-- Regression: FrameXML gaps closed via the stub-generation PIPELINE (real sources),
-- not via stubs/overrides. Requires --with-stubs. Each line below would emit a false
-- undefined-global / cannot-call / undefined-field if its pipeline fix regressed; the
-- exhaustive diagnostic checker fails the test on any uncovered diagnostic.
--
--   * Named <FontString>/<Texture> region globals — emitted from XML by the
--     stub_gen opener regex (src/stub_gen/mod.rs).
--   * ContainerFrameUtil_ConvertFilterFlagsToList — `X = nil; do X = function() end end`
--     now emitted as a real function (src/annotations/scan_globals.rs).
--   * EncounterJournal.instanceID/.encounterID — field writes onto a class-typed
--     global, registered by discover_runtime_fields (src/stub_gen/framexml.rs).
--   * Frame:SetMinResize/SetMaxResize — wiki {{widgetmethod removed=10.0.0}} pages
--     (src/stub_gen/wiki.rs), emitted Classic-flavored.
--   * frame.Child parentKey fields (e.g. WardrobeTransmogFrame.ToggleSecondary-
--     AppearanceCheckbox) — harvested from XML structure via xml_scan
--     (src/stub_gen/xml_frames.rs); the child's base widget type is kept so it
--     resolves even when an inherits= template is unavailable in the stubs.
--
-- Intentionally NOT closed (no fix is correct), so they are only mentioned here, not
-- referenced as code: AccountBankPanel and InterfaceOptionsFramePanelContainer are
-- defined only in FrameXML (absent from the wow-ui-source clone) so they stay
-- undefined-global; GenericTraitFrame:SetSystemID does not exist in any source (the
-- real method is SetConfigIDBySystemID) so the undefined-field is a true positive.

-- ── Named region globals (classic crafting/enchant FontStrings) ──────────────
local a = TradeSkillDescription
--        ^ hover: (global) TradeSkillDescription: FontString {
local b = TradeSkillReagentLabel:GetText()
local c = CraftDescription:GetText()
local d = CraftReagentLabel:GetText()

-- ── FrameXML util function previously typed nil (false cannot-call) ──────────
local g = ContainerFrameUtil_ConvertFilterFlagsToList(1)
--        ^ hover: (global) function ContainerFrameUtil_ConvertFilterFlagsToList(filterFlags)

-- ── EncounterJournal runtime field writes (registered from Blizzard Lua) ─────
local inst = EncounterJournal.instanceID
local enc = EncounterJournal.encounterID

-- ── Classic-only Frame widget methods (removed from retail in 10.0.0) ────────
local fr = CreateFrame("Frame")
fr:SetMinResize(1, 1)
fr:SetMaxResize(2, 2)

-- ── parentKey child field harvested from XML (frame.Child), base type resolves ─
local cb = WardrobeTransmogFrame.ToggleSecondaryAppearanceCheckbox
local checked = WardrobeTransmogFrame.ToggleSecondaryAppearanceCheckbox:GetChecked()
