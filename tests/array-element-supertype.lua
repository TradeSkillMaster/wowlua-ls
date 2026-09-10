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

-- Boolean-literal fields in inferred table constructors widen to `boolean`, just
-- as numeric/string literal fields already do (an inferred field is mutable). Two
-- constructors differing only in such a field therefore CONVERGE to a single shape
-- in the array's element union instead of surfacing as `{f: true} | {f: false}`.
local boolItems = {
  { isComplete = true, pct = 1, remaining = 2 },
  { isComplete = false, pct = 3, remaining = 4 },
}
local boolItemsRef = boolItems
--    ^ hover: (local) boolItemsRef: {isComplete: boolean, pct: number, remaining: number}[]

-- Control: string/number fields already behave this way — same single-shape result.
local strItems = {
  { kind = "a", n = 1 },
  { kind = "b", n = 2 },
}
local strItemsRef = strItems
--    ^ hover: (local) strItemsRef: {kind: string, n: number}[]

-- Guard against over-widening: an explicit per-field `---@type true` is intentional
-- and must be preserved (only bare, unannotated literals widen).
local tagged = {
  ready = true, ---@type true
  pct = 1,
}
local readyVal = tagged.ready
--    ^ hover: (local) readyVal: true

-- An `@as` cast is an equally explicit assertion and must also survive widening.
-- Here the cast value's natural type is `boolean`, so the literal `true` proves
-- the cast is honored rather than discarded.
---@return boolean
local function truthy() return 1 == 1 end
local castItem = {
  ready = truthy() --[[@as true]],
  pct = 1,
}
local castReady = castItem.ready
--    ^ hover: (local) castReady: true

-- ── Widening does not hide a *wrong* literal against a literal target ────────
-- Widening lets identical constructors converge (above), but when the target
-- field type is a specific literal the precise value is still recovered and
-- compared: an equal literal passes, a different one of the same family is a
-- genuine mismatch. Uniform across number, string, and boolean; both `@type`
-- assignment (`assign-type-mismatch`) and `@return` (`return-mismatch`). The
-- anonymous-shape target shows the widened field in its whole-shape message
-- (`{n: number}`); the paired correct case staying clean is what proves the
-- signal is the literal value, not the field's presence.
---@alias NumOne { n: 1 }
---@alias StrA { s: "a" }
---@alias BoolT { b: true }

---@return NumOne
local function retNumBad() return { n = 2 } end
--                                ^ diag: return-mismatch ~expected return type `{n: 1}`
---@return NumOne
local function retNumOk() return { n = 1 } end

---@type NumOne
local nBad = { n = 2 }
--           ^ diag: assign-type-mismatch ~(expected '{n: 1}')
---@type NumOne
local nOk = { n = 1 }

---@type StrA
local sBad = { s = "b" }
--           ^ diag: assign-type-mismatch ~(expected '{s: "a"}')
---@type StrA
local sOk = { s = "a" }

---@type BoolT
local bBad = { b = false }
--           ^ diag: assign-type-mismatch ~(expected '{b: true}')
---@type BoolT
local bOk = { b = true }

-- Number literals compare by value, not spelling: `2.0` and `0x2` equal `2`, so
-- an equivalent numeric spelling stays clean (no false positive).
---@alias NumTwo { n: 2 }
---@type NumTwo
local nFloat = { n = 2.0 }
---@type NumTwo
local nHex = { n = 0x2 }

-- A computed (non-literal) value against a literal target is NOT flagged — the
-- LS does not constant-fold, so only bare literals are compared precisely.
local function computeN() return 9 end
---@type NumOne
local nComputed = { n = computeN() }

-- Escape hatches survive: an explicit per-field `---@type` matching the target,
-- or an `@as` cast to the target literal, is honored rather than the raw value.
-- `n = 2` would `assign-type-mismatch` on its own (see `nBad` above); the
-- `---@type 1` overriding it to the target literal is what keeps this clean.
---@type NumOne
local escType = { n = 2, ---@type 1
}
---@type NumOne
local escAs = { n = (2) --[[@as 1]] }
-- But an `@as` to a *different* literal is an explicit wrong assertion → flagged.
---@type NumOne
local escAsBad = { n = (2) --[[@as 3]] }
--               ^ diag: assign-type-mismatch ~(expected '{n: 1}')

-- Regression: because a bare boolean-literal field widens to `boolean` (above), a
-- `boolean` field must still satisfy a target that wants the literal `true` — just
-- as a widened `number`/`string` field satisfies a numeric/string-literal target.
-- The motivating shape is an `@alias` intersecting an array with a boolean-tag shape
-- (`T[] & {done: true}`): the constructor `{ done = true }` infers `{done: boolean}`,
-- which must return-check against the tagged alias with NO return-mismatch.
---@alias TaggedArray string[] & {done: true}

---@return TaggedArray
local function packChunks()
  local out = { done = true }
  table.insert(out, "chunk")
  return out
end

-- Guard: the widening-to-literal-target rule above must not collapse `true` and
-- `false`. `{ done = false }` widens to `{done: boolean}`, but its literal value
-- is recovered and checked against the alias's `done: true`, so a bare wrong
-- literal still return-mismatches (no `@as` needed to force it).
---@return TaggedArray
local function packWrong()
  return { done = false }
  --     ^ diag: return-mismatch
end
