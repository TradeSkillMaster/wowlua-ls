---@diagnostic disable: unused-local
-- Cross-file body-inferred return type test: usage
-- Verifies that cross-file callers see the body-inferred return
-- types (arity and coarse types) even without @return annotations.

local addonName, ns = ...

-- Multi-return: (any, boolean)
local item, found = ns.Core.GetCachedItem("x")
--     ^ hover: (local) item: any  def: local
--           ^ hover: (local) found: boolean  def: local

-- Single boolean return
local has = ns.Core.HasItem("x")
--    ^ hover: (local) has: boolean  def: local

-- `not` expression → boolean
local empty = ns.Core.IsEmpty()
--    ^ hover: (local) empty: boolean  def: local

-- Literal returns: (string, number, true) — the engine preserves the boolean
-- literal `true` (matching the def-site), not the coarse `boolean`.
local d1, d2, d3 = ns.Core.GetDefaults()
--    ^ hover: (local) d1: string  def: local
--        ^ hover: (local) d2: number  def: local
--            ^ hover: (local) d3: true  def: local

-- Multi-path returns via correlated overloads
local val, ok = ns.Core.TryGet("x")
--    ^ hover: (local) val: any  def: local
--         ^ hover: (local) ok: boolean  def: local

-- Different concrete types at the same slot: the precise cross-file return is
-- the union `string | number` (the engine keeps both branch types).
local cls, cfound = ns.Core.Classify("x")
--    ^ hover: (local) cls: string | number  def: local
--          ^ hover: (local) cfound: boolean  def: local

-- Parenthesized comparison
local eq = ns.Core.CheckWrapped(1, 2)
--    ^ hover: (local) eq: boolean  def: local
