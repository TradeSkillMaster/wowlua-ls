-- The "global-is-also-a-@class" addon-namespace idiom: a global assigned from
-- `select(2, ...)` and tagged `@class`, with fields written across files via a
-- local alias. Fields whose value shape the coarse scan can't capture —
-- positional / bracket-keyed / empty table literals, and a bare reference to a
-- resolved class-typed global — must still resolve cross-file. Regression: they
-- were dropped as speculative placeholders by the prescan overlay-import filter,
-- false-positiving every cross-file read as `undefined-field`.
---@diagnostic disable: unused-local, unused-function

--- @class GcfHelper
GcfHelper = {}
function GcfHelper:Assist() end

--- @class GcfNs
GcfNs = select(2, ...)
