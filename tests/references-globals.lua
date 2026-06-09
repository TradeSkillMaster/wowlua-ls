---@diagnostic disable: unused-local, unused-function
-- Find-references behavior when local field/method names collide with real WoW
-- globals. Requires --with-stubs so the externals (e.g. `_G.GetText`) actually
-- exist; without them the field-first ordering in `reference_target_at` would
-- match what the old symbol-first code happened to produce.

-- Regression: a method name that collides with a real WoW global (`GetText` is
-- both a global localization helper and a common frame method) must not pull
-- every `:GetText()` call into the same reference set. Cursor on the method
-- name in `function RefLabel:GetText()` resolves to the method on RefLabel,
-- never to the external `_G.GetText`, so we only get RefLabel call sites.
---@class RefLabel
local RefLabel = {}
function RefLabel:GetText()
--                ^ refs: 14:19, 22:10
end
---@class RefOtherLabel
local RefOtherLabel = {}
function RefOtherLabel:GetText()
end
local refLabel = RefLabel
refLabel:GetText()
local refOther = RefOtherLabel
refOther:GetText()

-- Regression: `_G.X` and `local g = _G; g.X` are explicit ways to reach a
-- global, so find-references on the field-position `X` must fall through to
-- symbol lookup (via `is_g_dot_field`) and produce the same result as a bare
-- `X` lookup. Without the exception, the field-position guard would suppress
-- it entirely and references_at would return None.
local gtxt = GetText
--           ^ refs: 31:14
print(_G.GetText)
--       ^ refs: 31:14
local g = _G
print(g.GetText)
--      ^ refs: 31:14
