-- Array element type: common-supertype inference for heterogeneous arrays.
--
-- A table constructor whose element type is a most-specific intersection (e.g.
-- `Frame & InsetFrameTemplate` from a templated CreateFrame) must NOT pin the
-- array so strictly that inserting a less-specific sibling (a plain `Frame`) is
-- rejected. The element type checked at subsequent `table.insert` calls is
-- widened to the union of the intersection's facets — a common supertype — so a
-- value sharing any facet type-checks, while a genuinely-unrelated value (a
-- string, a number) shares no facet and is still flagged. The binding itself
-- stays precise, so `ipairs`/index reads keep the intersection's members.
---@diagnostic disable: unused-local, unused-function

local templated = CreateFrame("Frame", nil, nil, "InsetFrameTemplate")
local plain = CreateFrame("Frame")
local btn = CreateFrame("Button")

-- The motivating case: a single-element constructor pins the element type to
-- the first element's most-specific intersection `Frame & InsetFrameTemplate`.
local frames = {templated}

-- A plain Frame, a Button (both share the `Frame` facet), and another templated
-- frame must all type-check — no false `type-mismatch`.
table.insert(frames, plain)
table.insert(frames, btn)
table.insert(frames, CreateFrame("Frame", nil, nil, "InsetFrameTemplate"))

-- Genuinely-incompatible values share no facet and are still flagged.
table.insert(frames, "not a frame")
--                   ^ diag: type-mismatch
table.insert(frames, 42)
--                   ^ diag: type-mismatch

-- Reads keep the precise element type: iterating still resolves Frame methods,
-- so no false `undefined-field`/`cannot-call` on the loop variable.
for _, f in ipairs(frames) do
  f:SetPoint("TOP")
end

-- Multi-element constructor that already contains the plain supertype: inserting
-- the plain type was never the problem, but pin it down as a sanity check.
local mixed = {plain, templated}
table.insert(mixed, btn)

-- Negative control: a hand-written intersection parameter (NOT an array-bound
-- generic) is deliberately strict — a value missing one facet is still rejected,
-- proving the relaxation is scoped to array element inference.
---@class FacetA
---@field a number
---@class FacetB
---@field b number

---@param p FacetA & FacetB
local function needsBoth(p) end

---@param onlyA FacetA
local function useOnlyA(onlyA)
  needsBoth(onlyA)
  --        ^ diag: type-mismatch
end
