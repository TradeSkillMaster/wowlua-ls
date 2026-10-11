---@diagnostic disable: create-global, undefined-field
-- A constructor under a path the class doesn't have: its functions are not the
-- root's (`@accessor`) methods.
---@class Guarded
Guarded = {}
Guarded.Unknown.Helpers = {
    Probe = function() return 1 end,
}
