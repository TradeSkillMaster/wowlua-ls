-- Cross-file test: accessing namespace fields that were assigned from locals
local addonName, addon = ...

-- Should resolve to Texture (from CreateTexture return type)
local sa = addon.SurgeArc
--    ^ hover: (local) sa: Texture {  def: local

-- Should resolve to Frame (from CreateFrame with "Frame" arg)
local td = addon.TextDisplay
--    ^ hover: (local) td: Frame {  def: local

-- Should resolve to Texture (from CreateTexture return type)
local tb = addon.TextBackground
--    ^ hover: (local) tb: Texture {  def: local
