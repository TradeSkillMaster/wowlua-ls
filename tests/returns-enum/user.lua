---@diagnostic disable: unused-local

-- Members of `@returns-enum` results assigned in defs.lua, read from another file.
local _, addon = ...

local right = addon.Anchor.Right
--                         ^ hover: (field) Right: number  def: external
local on = addon.Mode.On
--                    ^ hover: (field) On: number = 1  def: external
local fast = GlobalSpeed.Fast
--                       ^ hover: (field) Fast: number  def: external

---@param mode AddonMode
local function setMode(mode) end
setMode(addon.Mode.Off)
setMode("off")
--      ^ diag: type-mismatch
local typo = addon.Mode.Of
--                      ^ diag: undefined-field

local frameKind = addon.Kinds.Frame
--                            ^ hover: (field) Frame: number  def: external
local sad = addon.Moods.Sad
--                      ^ hover: (field) Sad: number  def: external
local dark = addon.Shade.Dark
--                       ^ hover: (field) Dark: number = 2  def: external

---@enum UserMode
local UserMode = addon.MakeEnum("Up", "Down")
local up = UserMode.Up
--                  ^ hover: (field) Up: number = 1
local upTypo = UserMode.Upp
--                      ^ diag: undefined-field
