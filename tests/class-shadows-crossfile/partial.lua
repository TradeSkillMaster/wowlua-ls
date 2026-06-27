---@diagnostic disable: unused-local

-- A second, partial @class CVarInfo declared in a different file. Its fields merge
-- additively onto the same class (matching workspace partial-class semantics); the
-- optional `extra?` is not a required field, so the constructors in defs.lua stay
-- valid. It still warns, because it too reuses a built-in stub class name with an
-- explicit @field.
---@class CVarInfo
-- ^ diag: class-shadows-builtin
---@field extra? string
