---@diagnostic disable: unused-local, unused-function
-- "Mainline flavor" half: larger-arity twins of classic.lua's definitions. Both
-- files scan into one workspace; the merge keeps classic.lua's smaller-arity
-- definition (it sorts first) and drops these as unannotated duplicates. The
-- call sites live in user.lua (a third file that does NOT redefine them, so its
-- calls resolve against the merged cross-file signature — exactly the shape of
-- the reported Auctionator bug, where the call and the two definitions are in
-- three different files).
local _, ns = ...

-- Larger-arity twin of GetInfoText — two params.
function ns.CraftingInfo.GetInfoText(schematicForm, showProfit)
  return schematicForm, showProfit
end

-- Larger-arity twin of the colon method — two params.
function ns.Widget:CreateTerm(term, config)
  return term, config
end

-- Same-arity twin of Echo (also one param) — agrees with classic.lua, so the
-- two definitions are NOT in conflict.
function ns.SameArity.Echo(a)
  return a
end

-- Identical dot-with-explicit-self twin of classic.lua's Handler (same arity),
-- so the two are NOT a conflict — see classic.lua for the asymmetry it guards.
function ns.DotSelf.Handler(self, x)
  return x
end
