-- Cross-file overlay test: access fields defined on CreateFrame result via class field
-- Requires: --with-stubs

---@type FrameOverlayHost
local host = nil

-- Access overlay fields through class field indirection (CreateFrame path)
local retrieved = host.display
local _cf = retrieved.customField
--                    ^ hover: (field) customField: number  def: external

local _h = retrieved.handler
--                   ^ hover: (field) handler: function  def: external

local _txt = retrieved.Text
--                     ^ hover: (field) Text: FontString {  def: external

-- Access overlay fields through @type annotation path
---@type TypeAnnotatedHost
local thost = nil
local tframe = thost.frame
local _tf = tframe.typedField
--                 ^ hover: (field) typedField: string  def: external
