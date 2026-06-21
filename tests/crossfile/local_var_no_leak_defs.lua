-- Cross-file test: a local variable that is reassigned must NOT leak as a global.
-- The reassignment is a plain `value = ...` statement, which the coarse global
-- scan must recognize as a reassignment of the local (declared in a nested
-- do/for block) rather than an implicit global creation.
local private = select(2, ...)

local TABLE = {}

do
    for i = 0, 10 do
        local value = i
        local half = value % 2
        value = (value - half) / 2
        TABLE[i] = value
    end
end

private.TABLE = TABLE

-- An explicit `_G.marker` write must still create a global even though
-- `marker` is also declared as a local in this file.
local marker = "local-only" ---@diagnostic disable-line: unused-local
_G.marker = 99
