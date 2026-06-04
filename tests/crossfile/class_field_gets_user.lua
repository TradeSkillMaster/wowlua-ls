---@diagnostic disable: unused-local
-- Cross-file class field type test: accessing fields from another file

---@type CFGDisplay
local display

-- Top-level field from inherited method call
local ag = display.animGroup
--                 ^ hover: (field) animGroup: AnimationGroup  def: external
local _t = display.texture
--                 ^ hover: (field) texture: Texture  def: external

-- Top-level field from global function with string arg
local _f = display.frame
--                 ^ hover: (field) frame: Frame  def: external

-- Self-field from method body (inherited method call, no @type)
---@type CFGWidget
local widget

local wa = widget.anim
--                ^ hover: (field) anim: AnimationGroup  def: external
local wt = widget.tex
--                ^ hover: (field) tex: Texture  def: external
