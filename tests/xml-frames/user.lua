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
--     ^ hover: (local) _pf: PlayerInfoFrame {  diag: none

-- $parent-resolved globals exist
local _pic = PlayerInfoFrameContainer
--     ^ hover: (local) _pic: PlayerInfoFrameContainer {  diag: none
local _pii = PlayerInfoFrameIcon
--     ^ hover: (local) _pii: PlayerInfoFrameIcon {  diag: none

-- Intrinsic element creates proper class
---@type CustomButton
local cb
--    ^ hover: (local) cb: CustomButton

-- Intrinsic usage creates proper global
local _sb = SpecialButton
--     ^ hover: (local) _sb: SpecialButton {  diag: none

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
--    ^ hover: (local) abtn: MyButtonTemplate & MyBaseMixin
-- Mixin-only child: intersection of base element type + mixin
local extra = host.Extra
--    ^ hover: (local) extra: Frame & MyBaseMixin

-- parentArray with inherits: array element type uses template
---@type ListFrame
local lf
local items = lf.Items
--    ^ hover: (local) items: MyBaseTemplate[]

-- Hyphenated names should not create globals (invalid Lua identifier)
local x = InvalidFrame
--        ^ diag: undefined-global
