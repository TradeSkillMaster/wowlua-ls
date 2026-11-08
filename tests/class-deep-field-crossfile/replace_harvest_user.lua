---@diagnostic disable: unused-local

-- An entry a function replaces with a value the scan can't type reads as the
-- defining file reads it.
local cache = DeepFieldReplaced.cache
--    ^ hover: (local) cache: { size: number }

-- An entry deep writes extend and a write of the field, as the defining file
-- reads it.
local db = DeepFieldReplaced.db
--    ^ hover: (local) db: { seen: true } | DeepFieldStore
