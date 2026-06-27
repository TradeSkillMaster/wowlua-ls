---@diagnostic disable: unused-local

-- This library file reuses the built-in `Frame` class name with its own `@field`.
-- Workspace `@class` declarations that collide with a stub class merge ADDITIVELY
-- onto the stub (the stub table is never replaced), so the built-in `Frame` keeps
-- all its fields workspace-wide and this library's field is added on top. A
-- vendored library therefore can't silently strip a built-in type from other
-- files. (No `class-shadows-builtin` warning surfaces from a library file — its
-- diagnostics are suppressed.)
---@class Frame
---@field libField string
local L = {}
return L
