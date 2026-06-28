---@diagnostic disable: unused-local, unused-function
-- "Classic flavor" half of a flavor-split addon. The same namespaced function
-- and colon method are defined here with SMALLER arities than in mainline.lua.
-- Because the language server merges every file into one workspace, the merge
-- keeps whichever definition registers first and drops the other (unannotated)
-- duplicate. `call_arity` must therefore NOT flag the other flavor's call sites
-- against the one surviving definition (see pre_globals `conflicting_arity_funcs`).
local _, ns = ...

-- Dotted namespace function — zero params (cf. Auctionator's GetInfoText, which
-- is `function Auctionator.CraftingInfo.GetInfoText()` in Source_Classic but
-- `(schematicForm, showProfit)` in Source_Mainline). Unannotated on purpose:
-- the merge only drops *unannotated* duplicates, which is the bug's trigger.
function ns.CraftingInfo.GetInfoText()
  return "classic"
end

-- Colon method on an addon-ns sub-table — one param. Exercises the self-offset
-- path of the conflicting-arity skip.
function ns.Widget:CreateTerm(term)
  return term
end

-- Negative control: defined only ONCE (no cross-flavor twin), so it must still
-- be arity-checked normally — an over-call is a genuine error.
function ns.Solo.Only(a)
  return a
end

-- Negative control: also one param in mainline.lua. Two definitions that AGREE
-- on arity are NOT a conflict, so an over-call must still warn.
function ns.SameArity.Echo(a)
  return a
end

-- Negative control for the dot-with-explicit-self asymmetry: a DOT-defined
-- method whose param list explicitly includes `self` (the scanner only strips
-- `self` from *colon* methods, so `self` survives in the dot case). Defined
-- identically — same arity — in mainline.lua, so the two are NOT a conflict and
-- arity checking must stay active. Guards `duplicate_def_arity_conflicts`
-- against counting `self` on one side only.
function ns.DotSelf.Handler(self, x)
  return x
end
