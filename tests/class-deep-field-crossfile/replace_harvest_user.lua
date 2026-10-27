---@diagnostic disable: unused-local

-- An entry a function replaces with a value the scan can't type reads as the
-- defining file reads it.
local cache = DeepFieldReplaced.cache
--    ^ hover: (local) cache: { size: number }
