-- Cross-file test: access fields assigned from method call locals
-- Requires: --with-stubs

---@type WidgetPanel
local panel = nil

-- Fields assigned from locals initialized by method calls should resolve cross-file
local _bg = panel.Background
--                ^ hover: (field) Background: Texture {  def: external

local _flash = panel.Flash
--                   ^ hover: (field) Flash: Texture {  def: external

-- Direct method call result field also resolves
local _dt = panel.DirectTexture
--                ^ hover: (field) DirectTexture: Texture {  def: external
