---@diagnostic disable: unused-local, unused-function
-- Defining file for the cross-file body-derived multi-return arity test.
-- None of these functions carry a `@return` annotation: their arity is
-- harvested from the body by the deferred cross-file resolution path
-- (analysis/deferred.rs) so callers in user.lua see the real multi-return
-- arity rather than a collapsed single value.
local _, ns = ...

-- Plain global, three literal returns.
function GetTriple() return 1, 2, 3 end

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
