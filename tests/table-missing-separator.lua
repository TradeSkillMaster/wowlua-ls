---@diagnostic disable: unused-local
-- Regression: a missing separator (comma) inside a table constructor must not
-- abandon the table and reinterpret the remaining `Name = {}` entries as
-- top-level global assignments. That misparse surfaced as a cascade of spurious
-- `create-global` / `undefined-global` diagnostics on the lower half of a table
-- while it was momentarily malformed during editing (the reported bug).
--
-- The deliberate missing comma below is a syntax error, which the harness
-- exempts from exhaustive checking. The actual guarantee is verified by the
-- harness's exhaustive checking: NONE of the entries after the missing comma
-- may produce a (non-syntax) diagnostic such as `create-global`.

local marker = 5
--    ^ hover: (local) marker: number = 5

local Registry = {
    First = {},
    Nested = {
        1, 2, 3,
    }
    AfterNested = {},
    Another = {},
    Third = {},
}

-- A positional anonymous-function field value must also recover across a
-- dropped separator (the `function(` keyword begins a field expression, not a
-- statement), so the entries after it stay fields and emit no create-global.
local Handlers = {
    1
    function() return 2 end,
    Last = {},
}

return Registry, Handlers
