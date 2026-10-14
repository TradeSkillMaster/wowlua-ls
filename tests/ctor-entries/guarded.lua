---@diagnostic disable: create-global, undefined-field
-- A constructor under a path the class doesn't have: its functions are not the
-- root's (`@accessor`) methods.
---@class Guarded
Guarded = {}
Guarded.Unknown.Helpers = {
    Probe = function() return 1 end,
}
-- Nor the methods of a local table exported there.
local Helpers = {}
function Helpers.Lookup() return 1 end
Guarded.Missing.Helpers = Helpers
