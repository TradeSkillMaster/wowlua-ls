---@diagnostic disable: unused-local
--- @class GcfNs
local NS = GcfNs
NS.positional = { "a", "b" }               -- positional array (no identifier keys)
NS.bracketInt = { [1] = "x", [5] = "y" }   -- bracket-integer keys
NS.empty = {}                              -- empty table
NS.ref = GcfHelper                         -- bare ref to a resolved @class global
NS.map = { alpha = 1, beta = 2 }           -- identifier keys (control: already worked)
