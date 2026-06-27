---@diagnostic disable: unused-local, unused-function
-- Requires: --with-stubs
-- Regression coverage for the wrong-arity stub fixes. Every call/destructure
-- below matches the corrected signature, so the exhaustive diagnostic checker
-- fails if any previously-emitted redundant-parameter / missing-parameter /
-- unbalanced-assignments warning reappears. A few hover assertions additionally
-- pin the corrected return shapes.

-- Trailing `...` params dropped by the InferredReturns scan before the fix
-- (extract_inferred_return now re-appends the vararg, so the extra args are not
-- flagged redundant-parameter).
local closure = GenerateClosure(print, 1, 2, 3)

local frame = CreateFrame("Frame")
CallMethodOnNearestAncestor(frame, "Method", 1, 2)

local formatted = LinkUtil.FormatLink("item", "Display", 1, 2, 3)

-- LinkUtil.ExtractLink tail-calls string.match (a vararg-returning function), so
-- the generated stub carries a `...string` vararg return instead of collapsing to
-- a single value — destructuring multiple captures must not over-assign.
local linkType, linkData, displayText = LinkUtil.ExtractLink("|Hitem:1|h[x]|h")
--    ^ hover: (local) linkType: string

-- ScrollBoxLinearViewMixin:SetPadding(top, bottom, left, right, spacing): the
-- mixin inherits SetPadding from two parents; runtime (and now the stub) uses the
-- 5-arg ScrollBoxLinearBaseViewMixin form, not the 1-arg ScrollBoxViewMixin one.
local view = CreateScrollBoxLinearView()
view:SetPadding(5, 5, 0, 0, 0)

-- string.find returns start, end, then a vararg of captures (Ketho's
-- `@return any|nil ... captured` now parses as a variadic return), so
-- destructuring the two captures of a 2-group pattern must not over-assign.
local s, e, cap1, cap2 = string.find("ab cd", "(%w+) (%w+)")
--          ^ hover: (local) cap1: any?

-- `@return <type> ... <name>` (Ketho's vendor vararg form) parses as a variadic
-- return that fills the remaining destructure slots.
---@return number ... values
local function multiReturn() return 1, 2, 3 end
local v1, v2, v3, v4 = multiReturn()
--                ^ hover: (local) v4: number
