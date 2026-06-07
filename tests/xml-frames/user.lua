---@diagnostic disable: unused-local
-- Tests for XML frame/template scanning

-- Virtual template creates a class with the right parents and fields
---@type MyBaseTemplate
local base
--    ^ hover: (local) base: MyBaseTemplate {

-- parentKey children become typed fields
local bg = base.Background
--    ^ hover: (local) bg: Texture
local title = base.Title
--    ^ hover: (local) title: FontString
local content = base.ContentFrame
--    ^ hover: (local) content: Frame

-- KeyValue fields resolve to correct types
local lbl = base.label
--    ^ hover: (local) lbl: string
local cnt = base.count
--    ^ hover: (local) cnt: number
local en = base.enabled
--    ^ hover: (local) en: boolean

-- Template inheriting another template gets inherited fields + own fields
---@type MyButtonTemplate
local btn
local nt = btn.NormalTexture
--    ^ hover: (local) nt: Texture
local glow = btn.Glow
--    ^ hover: (local) glow: Texture
local fade = btn.FadeAnim
--    ^ hover: (local) fade: AnimationGroup

-- Non-virtual frame creates a global (no undefined-global)
local _pf = PlayerInfoFrame
--     ^ hover: (local) _pf: PlayerInfoFrame {

-- $parent-resolved globals exist
local _pic = PlayerInfoFrameContainer
--     ^ hover: (local) _pic: PlayerInfoFrameContainer {
local _pii = PlayerInfoFrameIcon
--     ^ hover: (local) _pii: PlayerInfoFrameIcon {

-- Intrinsic element creates proper class
---@type CustomButton
local cb
--    ^ hover: (local) cb: CustomButton

-- Intrinsic usage creates proper global
local _sb = SpecialButton
--     ^ hover: (local) _sb: SpecialButton {

-- Top-level texture template creates a class
---@type WoodTileTemplate
local wt
--    ^ hover: (local) wt: WoodTileTemplate

-- Top-level animation group template creates a class with fields
---@type FadeInTemplate
local fi
local aa = fi.AlphaAnim
--    ^ hover: (local) aa: Animation

-- Child inheriting a template: field type uses template (not base element type),
-- since the template already inherits from the base element type.
---@type HostFrame
local host
local panel = host.Panel
--    ^ hover: (local) panel: MyBaseTemplate {
-- Template fields accessible through inherited parentKey field
local panelBg = host.Panel.Background
--    ^ hover: (local) panelBg: Texture
local panelTitle = host.Panel.Title
--    ^ hover: (local) panelTitle: FontString
-- Child with both inherits and mixin: intersection of template + mixin
local abtn = host.ActionBtn
--    ^ hover: (local) abtn: MyButtonTemplate {
-- Mixin-only child: intersection of base element type + mixin
local extra = host.Extra
--    ^ hover: (local) extra: MyBaseMixin {

-- parentArray with inherits: array element type uses template
---@type ListFrame
local lf
local items = lf.Items
--    ^ hover: (local) items: MyBaseTemplate[]

-- parentKey fields are visible on mixin classes: SearchMixin methods can
-- access self.InputBox / self.SearchButton without undefined-field.
---@class SearchMixin
local SearchMixin = {}

function SearchMixin:OnLoad()
    self.InputBox:SetText("")
    --   ^ hover: (field) InputBox: EditBox
    self.SearchButton:Enable()
    --   ^ hover: (field) SearchButton: Button
end

-- Nested parentKey propagation: unnamed child frames with parentKey should
-- propagate their nested fields into the parent's field type without needing
-- user @class annotations. DialogFrame.Sidebar is typed as
-- MyBaseTemplate & {ActionBtn: Button} from XML alone.
---@class DialogFrameMixin
local DialogFrameMixin = {}

function DialogFrameMixin:Init()
    -- Sidebar comes from XML parentKey, ActionBtn is a nested parentKey inside it
    self.Sidebar.ActionBtn:Enable()
    --           ^ hover: (field) ActionBtn: Button
    -- Template fields are still accessible through the base type
    self.Sidebar.Title:SetText("test")
    --           ^ hover: (field) Title: FontString
end

-- User @class overrides XML-generated field types: the XML scanner infers
-- MyPanel.Header as MyBaseTemplate, but the user's @class MyPanel defines
-- @field Header as MyPanelHeader (with additional fields like CloseBtn).
---@class MyPanelHeader : Frame
---@field CloseBtn Button

---@class MyPanel : Frame
---@field Header MyPanelHeader
MyPanelMixin = {}
-- ^ diag: create-global

function MyPanelMixin:DoSomething()
    -- self.Header should resolve to MyPanelHeader (user annotation), not
    -- MyBaseTemplate (XML-inferred type)
    local hdr = self.Header
    --    ^ hover: (local) hdr: MyPanelHeader
    self.Header.CloseBtn:Enable()
    --          ^ hover: (field) CloseBtn: Button
end

-- Leaf region elements with inherits: base region type is preserved.
-- FontString inherits a Font object, not a FontString template, so the
-- field should still be typed as FontString (not the Font object).
---@type StyledFrame
local styled
local slbl = styled.Label
--    ^ hover: (local) slbl: FontString
local sico = styled.Icon
--    ^ hover: (local) sico: Texture

-- Hyphenated names should not create globals (invalid Lua identifier)
local x = InvalidFrame
--        ^ diag: undefined-global
