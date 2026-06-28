---@diagnostic disable: unused-local
-- Consumer file. It only CALLS the flavor-split functions — it does not define
-- them — so every call resolves against the merged cross-file signature, which
-- the workspace fixed to classic.lua's smaller-arity definition. Before the fix
-- these calls drew a false `redundant-parameter`; the exhaustive harness fails
-- on any unasserted diagnostic, so the absence of `diag:` lines here is the
-- assertion that they are now clean.
local _, ns = ...

-- Two-arg call to the flavor-split namespace function. Valid for the mainline
-- definition; must NOT warn against the merged-in zero-param classic definition.
ns.CraftingInfo.GetInfoText(1, true)

-- Two-arg call to the flavor-split colon method (method-call / self-offset path).
ns.Widget:CreateTerm("x", {})

-- Negative control: a function with a single definition is still arity-checked,
-- so over-calling it is a genuine error.
ns.Solo.Only(1, 2, 3)
--           ^ diag: redundant-parameter

-- Negative control: two definitions that AGREE on arity are not a conflict, so
-- an over-call still warns (the skip is gated on *differing* arity, not on the
-- mere presence of a duplicate definition).
ns.SameArity.Echo(1, 2)
--                ^ diag: redundant-parameter

-- Negative control: a dot-defined method with an explicit `self` param, defined
-- identically in both files (2 params each, including self), is NOT a conflict.
-- The scanner keeps `self` in a dot method's param list, so an arity comparison
-- that strips `self` on only one side would mis-flag a conflict and silently
-- suppress this genuine over-call (4 args vs the 2-param signature).
ns.DotSelf.Handler(1, 2, 3, 4)
--                 ^ diag: redundant-parameter
