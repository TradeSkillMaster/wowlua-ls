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

-- parentArray creates array-typed field
---@type ListFrame
local lf
local items = lf.Items
--    ^ hover: (local) items: Frame[]

-- Hyphenated names should not create globals (invalid Lua identifier)
local x = InvalidFrame
--        ^ diag: undefined-global
