---@diagnostic disable: unused-local
-- A global created only inside a function body in the defs file must resolve
-- here without an undefined-global false positive.
local d = MY_SAVED_DATA
--        ^ hover: (global) MY_SAVED_DATA: ?  def: external

-- A function-scoped local in the defs file must NOT leak as a global: reading
-- it here is genuinely undefined.
local s = scratch + 1
--        ^ diag: undefined-global

-- An explicit `_G.EXPLICIT_GLOBAL = ...` inside a function body must register
-- the global even though `EXPLICIT_GLOBAL` is also a local in the defs file.
local eg = EXPLICIT_GLOBAL
--         ^ hover: (global) EXPLICIT_GLOBAL: ?  def: external
