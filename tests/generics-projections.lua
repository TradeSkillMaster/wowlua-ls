-- Test: `params<F>` and `returns<F>` utility-type projections.

---@class GenericRegistry<F>
local GenericRegistry = {}

---@generic F
---@param self GenericRegistry<F>
---@param key string
---@return returns<F>
function GenericRegistry:Peek(key) end

---@generic F
---@param self GenericRegistry<F>
---@param key string
---@param ... params<F>
---@return returns<F>
function GenericRegistry:Call(key, ...) end

---@class GPFrame
---@field id number
local GPFrame = {}

-- ── Baseline: returns<F> resolves to F's return type ────────────────────────

---@type GenericRegistry<fun(name: string): GPFrame>
local frameReg = {}

local peek = frameReg:Peek("k")
--    ^ hover: (local) peek: GPFrame {

-- ── Baseline: params<F> positional validation ───────────────────────────────

---@type GenericRegistry<fun(name: string, count: number)>
local argReg = {}

-- Correct args: no diag.
argReg:Call("k", "Shou", 3)
--          ^ diag: none
--                ^ diag: none
--                        ^ diag: none

-- Wrong type at vararg position: type-mismatch.
argReg:Call("k", 42, 3)
--               ^ diag: type-mismatch

-- Missing args: missing-parameter.
argReg:Call("k")
--    ^ diag: missing-parameter

-- Too many args: redundant-parameter.
argReg:Call("k", "a", 1, "extra")
--                       ^ diag: redundant-parameter

-- ── Unbound F: hover at class declaration shows raw projection ──────────────

function GenericRegistry:CallDecl(key, ...) end
--                       ^ hover: (method) function GenericRegistry:CallDecl(key, ...)

-- ── Multi-return F: returns<F> fires multi-return-projection warning ────────

---@type GenericRegistry<fun(k: string): string, boolean>
local multiReg = {}

local truncated = multiReg:Peek("k")
--                         ^ diag: multi-return-projection

-- Sanity: first-column type is still picked.
--    ^ hover: (local) truncated: string

-- ── Tuple-union F: same warning ─────────────────────────────────────────────

---@type GenericRegistry<fun(k: string): (number, boolean) | (nil, string)>
local tupleReg = {}

local tupleOut = tupleReg:Peek("k")
--                        ^ diag: multi-return-projection
--    ^ hover: (local) tupleOut: number | nil

-- ── Malformed: params<F> in non-vararg position ─────────────────────────────

---@class BadPosClass<F>
local BadPosClass = {}

---@generic F
---@param self BadPosClass<F>
---@param bogus params<F>
function BadPosClass:Bogus(bogus) end
--                          ^ diag: malformed-annotation

-- ── Malformed: params<F> in @return ─────────────────────────────────────────

---@class BadRetClass<F>
local BadRetClass = {}

---@generic F
---@param self BadRetClass<F>
---@return params<F>
function BadRetClass:BogusRet() end
--                    ^ diag: malformed-annotation

-- ── Malformed: projection shape errors ──────────────────────────────────────

---@generic F
---@param self BadPosClass<F>
---@return returns<returns<F>>
function BadPosClass:Circular() end
--                    ^ diag: malformed-annotation

---@generic F
---@param self BadPosClass<F>
---@return returns<NotAGeneric>
function BadPosClass:BadGeneric() end
--                    ^ diag: malformed-annotation

-- ── Unbound F at a call site: projections degrade gracefully ────────────────

-- When F is not bound (no @type on the receiver), projections don't fire.
-- Caller gets no extra diagnostics; return type falls back to `any`.
---@param anyReg GenericRegistry
local function usingUnbound(anyReg)
    anyReg:Call("k", 1, "s", 5)
    --    ^ diag: none
end

_G.useProjections = { frameReg, argReg, multiReg, tupleReg, BadPosClass, BadRetClass, peek, truncated, tupleOut, usingUnbound }
