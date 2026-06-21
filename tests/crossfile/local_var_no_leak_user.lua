-- Cross-file test: a reassigned local from another file must not be visible
-- as a bare global here. Regression for locals leaking via plain `x = ...`
-- reassignments inside nested do/for blocks.
local private = select(2, ...)

-- `value` is only ever a local in the defs file; it must be undefined here.
local x = value + 1 ---@diagnostic disable-line: unused-local
--        ^ diag: undefined-global

-- The namespace field assigned from the local table still resolves.
local _ = private.TABLE

-- An explicit `_G.marker = 99` in the defs file must create a global
-- even though `marker` is also a local there.
local m = marker + 1 ---@diagnostic disable-line: unused-local
--        ^ hover: (global) marker: number = 99
