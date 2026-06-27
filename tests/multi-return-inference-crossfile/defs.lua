---@diagnostic disable: unused-local, unused-function
-- Defining file for the cross-file body-derived multi-return arity test.
-- None of these functions carry a `@return` annotation: their arity is
-- harvested from the body by the deferred cross-file resolution path
-- (analysis/deferred.rs) so callers in user.lua see the real multi-return
-- arity rather than a collapsed single value.
local _, ns = ...

-- Plain global, three literal returns.
function GetTriple() return 1, 2, 3 end

-- Deferred global whose trailing return is a dynamic `table<K,V>` position: the
-- harvested arity (3) is only a lower bound, so a cross-file caller may
-- over-destructure without an `unbalanced-assignments` false positive (the
-- cross-file counterpart of the same-file dynTail/parseLike cases in
-- tests/multi-return-inference.lua).
function ParseDynamic()
  ---@type table<number, any>
  local t = {}
  return 1, t[15], t[17] or 0
end

-- *Authored* `@return` whose trailing slot is `any`. Unlike ParseDynamic, this
-- is NOT a body-harvested arity — the explicit annotation is an authoritative
-- contract (the function is absent from `deferred_returns`), so over-destructure
-- still warns. Locks in the `deferred_returns` membership gate that
-- distinguishes a harvested trailing `any` from an authored one.
---@param x any
---@return number
---@return any
function AnnotatedAny(x) return 1, x end

-- Global with correlated set-or-nil branches (synthesized return-only
-- overloads); both branches are arity 2.
---@param x boolean
function GetPairOrNil(x)
  if x then return 10, 20 end
  return nil, nil
end

-- Method on the addon namespace, branches of differing arity (widest = 3).
---@class XfModule
local M = {}
ns.Module = M

---@param id number
function M:Lookup(id)
  if id then return true, "found", 5 end
  return false
end
