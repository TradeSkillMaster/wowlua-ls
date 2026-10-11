---@diagnostic disable: unused-local
-- Regression: Blizzard virtual templates missing from the vendored annotations (newer
-- retail ones, Glue/Store UI, every non-retail branch's own) get classes generated
-- from wow-ui-source XML (src/stub_gen/xml_frames.rs). Requires --with-stubs. Before,
-- a frame created from one typed as `Frame & any`.

-- ── Retail template: mixin methods and child keys ────────────────────────────
local tab = CreateFrame("Button", nil, UIParent, "SocialUITabTemplate")
--    ^ hover: (local) tab: SocialUITabTemplate {
tab:SetTabEnabled(true)
local count = tab.Count
--    ^ hover: (local) count: SocialUITabTemplate_Count {
tab.Count:SetTxet("1")
--        ^ diag: undefined-field

-- ── Glue UI template ─────────────────────────────────────────────────────────
local glue = CreateFrame("Button", nil, UIParent, "GlueButtonTemplate")
--    ^ hover: (local) glue: GlueButtonTemplate {

-- ── Classic-only template ────────────────────────────────────────────────────
local title = CreateFrame("Button", nil, UIParent, "QuestLogTitleButtonTemplate")
--    ^ hover: (local) title: QuestLogTitleButtonTemplate {
local check = title.check
--    ^ hover: (local) check: Texture {

-- ── A class inheriting a template ────────────────────────────────────────────
---@class PreviewTab : Button, SocialUITabTemplate
---@type PreviewTab
local preview
local previewCount = preview.Count
--    ^ hover: (local) previewCount: SocialUITabTemplate_Count {
