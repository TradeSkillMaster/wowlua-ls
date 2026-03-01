-- Cross-file test: file B uses a different variable name but sees file A's fields
local addonName, addon = ...
local v = addon.version
--    ^ hover: v: number  def: local
local t = addon.title
--    ^ hover: t: string  def: local
