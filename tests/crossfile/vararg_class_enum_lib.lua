-- Cross-file test: @class on vararg-destructured local + @enum field assignment
-- Reproduces the pattern: local name, AddOn = ... ; ---@class Foo \n AddOn = GetAddon()
local name, VarargAddon = ...
---@class VarargAddonClass
VarargAddon = LibStub("AceAddon-3.0"):GetAddon(name)

---@enum VarargDifficulty
VarargAddon.Difficulty = {
    Normal = 1,
    Heroic = 2,
    Mythic = 23
}
