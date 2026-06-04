-- Cross-file class field pipeline stress test: consumer side.
-- Verifies every RHS pattern resolves correctly from another file.
-- Tests BOTH access paths: @type (external table) and @class re-declaration (prescan import).
-- Requires: --with-stubs

-- ── Path A: @type access (external table lookup) ───────────────────

---@type PipelineWidget
local w = nil

-- Pattern 1: literal number
local _n = w.litNumber
--           ^ hover: (field) litNumber: number  def: external

-- Pattern 2: literal string
local _s = w.litString
--           ^ hover: (field) litString: string  def: external

-- Pattern 3: literal boolean
local _b = w.litBool
--           ^ hover: (field) litBool: boolean  def: external

-- Pattern 4: function literal
local _f = w.litFunc
--           ^ hover: (field) litFunc: function  def: external

-- Pattern 5: table constructor
local _t = w.litTable
--           ^ hover: (field) litTable  def: external

-- Pattern 6: direct method call
local _dm = w.directMethodCall
--            ^ hover: (field) directMethodCall: Texture {  def: external

-- Pattern 7: direct global function call
local _dg = w.directGlobalCall
--            ^ hover: (field) directGlobalCall: Frame {  def: external

-- Pattern 8: indirect via local from method call
local _im = w.indirectMethodLocal
--            ^ hover: (field) indirectMethodLocal: Texture {  def: external

-- Pattern 9: indirect via local from global call
local _ig = w.indirectGlobalLocal
--            ^ hover: (field) indirectGlobalLocal: Frame {  def: external

-- Pattern 10: indirect via local from chained method call
local _fs = w.indirectFontString
--            ^ hover: (field) indirectFontString: FontString {  def: external

-- Pattern 11: nested sub-table field
local _it = w.SubPanel.InnerTexture
--                      ^ hover: (field) InnerTexture: Texture {  def: external

-- ── Path B: @class re-declaration (prescan import) ─────────────────

---@class PipelineWidget
local w2 = {}

function w2:UseFields()
    -- Verify fields survive prescan import into re-declared class
    self.indirectMethodLocal:Show()
    self.indirectGlobalLocal:Show()
    self.directMethodCall:Show()
end
