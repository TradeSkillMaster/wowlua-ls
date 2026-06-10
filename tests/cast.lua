---@diagnostic disable: undefined-global, unused-function, unused-local, redundant-return-value, unbalanced-assignments
-- Tests for @cast and @as annotations

-- ── @cast Replace ──────────────────────────────────────────────────────────────

---@type string|number|nil
local x = nil

---@cast x string
print(x)
--    ^ hover: (local) x: string  def: local

-- ── @cast Add ──────────────────────────────────────────────────────────────────

---@type string
local y = "hello"

---@cast y +number
print(y)
--    ^ hover: (local) y: string | number  def: local

-- ── @cast Remove ───────────────────────────────────────────────────────────────

---@type string|number|nil
local z = nil

---@cast z -nil
print(z)
--    ^ hover: (local) z: string | number  def: local

-- ── @cast Remove from non-union ────────────────────────────────────────────────

---@type string|nil
local w = nil

---@cast w -nil
print(w)
--    ^ hover: (local) w: string

-- ── @as inline expression cast ─────────────────────────────────────────────────

local a = nil --[[@as string]]
print(a)
--    ^ hover: (local) a: string  def: local

-- ── @cast with inline block comment syntax ─────────────────────────────────────

---@type any
local c = nil

--[[@cast c number]]
print(c)
--    ^ hover: (local) c: number

-- ── @as on field access in return statement ──────────────────────────────────

---@class AsReturnTarget
---@field cache AsReturnTarget

---@return string
function AsReturnTarget:GetCached()
    return self.cache --[[@as string]]
end

-- ── @cast malformed diagnostics ────────────────────────────────────────────────

---@cast
-- ^ diag: malformed-annotation

---@cast x
-- ^ diag: malformed-annotation

-- ── @cast inside function should not leak to parameter type ──────────────────

---@class CastBase
---@field foo number

---@class CastChild : CastBase
---@field bar string

---@param p CastBase
---@return boolean
local function castInsideFn(p)
    ---@cast p CastChild
    return p.bar == "x"
end

---@type CastBase
local cb = { foo = 1 }
castInsideFn(cb)

-- ── @cast add type already present (idempotent) ─────────────────────────────

---@type string|number
local dup = "hello"

---@cast dup +string
print(dup)
--    ^ hover: (local) dup: string | number  def: local

-- ── @cast remove type not in the union (no-op) ──────────────────────────────

---@type string|number
local noop = "hello"

---@cast noop -boolean
print(noop)
--    ^ hover: (local) noop: string | number  def: local

-- ── @cast remove non-nil from union ─────────────────────────────────────────

---@type string|number|boolean
local strip = "hello"

---@cast strip -number
print(strip)
--    ^ hover: (local) strip: string | boolean  def: local

-- ── @cast multiple consecutive casts ────────────────────────────────────────

---@type string|number|boolean|nil
local multi = nil

---@cast multi -nil
---@cast multi -boolean
print(multi)
--    ^ hover: (local) multi: string | number  def: local

-- ── @cast replace with class type ───────────────────────────────────────────

---@class CastTarget
---@field value number

---@type any
local obj = nil

---@cast obj CastTarget
print(obj.value)
--        ^ hover: (field) value: number

-- ── @cast add then remove ───────────────────────────────────────────────────

---@type string
local addrem = "hello"

---@cast addrem +number
---@cast addrem -string
print(addrem)
--    ^ hover: (local) addrem: number  def: local

-- ── @as on method call result should not trigger cannot-call ────────────────

---@class AsMethodCache
---@field GetValue fun(self: AsMethodCache, key: string): number | string | nil

---@type AsMethodCache
local asCache = {}

local asResult = asCache:GetValue("name") --[[@as string?]]
--       ^ hover: (local) asResult: string?  def: local

local _ = asResult

-- ── @cast inside elseif block ────────────────────────────────────────────────

---@type string|number|nil
local evar = nil
local etype = "test"

if etype == "foo" then
    print(evar)
elseif etype == "bar" then
    ---@cast evar string
    print(evar)
--        ^ hover: (local) evar: string
elseif etype == "baz" then
    ---@cast evar number
    print(evar)
--        ^ hover: (local) evar: number
end

-- ── @cast with unknown type (undefined-doc-name) ─────────────────────────────

---@type any
local unknownCast = nil

---@cast unknownCast NonExistentType
--^ diag: undefined-doc-name
print(unknownCast)

-- ── @cast add with unknown type ───────────────────────────────────────────────

---@type string
local addUnknown = "hello"

---@cast addUnknown +GhostType
--^ diag: undefined-doc-name
print(addUnknown)

-- ── @cast remove with unknown type ────────────────────────────────────────────

---@type string|number
local remUnknown = "hello"

---@cast remUnknown -PhantomType
--^ diag: undefined-doc-name
print(remUnknown)

-- ── @cast with known class type (no diagnostic) ───────────────────────────────

---@class CastKnown
---@field x number

---@type any
local knownCast = nil

---@cast knownCast CastKnown
print(knownCast)

-- ── @cast with block comment syntax and unknown type ──────────────────────────

---@type any
local blockCast = nil

--[[@cast blockCast BlockGhostType]]
--                   ^ diag: undefined-doc-name
print(blockCast)

-- ── @as go-to-definition on class type ──────────────────────────────────────

---@class AsDefClass
---@field value number

local asDefVar = nil --[[@as AsDefClass]]
--                           ^ def: local 231:1  hover: (class) AsDefClass

-- ── @cast (block comment) go-to-definition on class type ────────────────────

---@type any
local blockDefVar = nil

--[[@cast blockDefVar AsDefClass]]
--                    ^ def: local 231:1  hover: (class) AsDefClass

-- ── @cast (line comment) go-to-definition on class type ─────────────────────

---@type any
local lineDefVar = nil

---@cast lineDefVar AsDefClass
--                  ^ def: local 231:1  hover: (class) AsDefClass

-- ── @cast with deferred class-eq sibling narrowing (regression) ───────────
-- When a multi-return function uses tuple-union @return with a class-typed
-- discriminant, and the function is forward-declared (resolved in Phase 2),
-- @cast on a sibling should take precedence over deferred OverloadNarrow.

---@class CastErrKind
---@field MIN number
---@field MAX number

---@type CastErrKind
local CEK = { MIN = 1, MAX = 2 }

local castNs = {}

---@param str string
---@return number
local function castNeedsStr(str)
    return #str
end

local function castMultiRetTest()
    local ok, errKind, errVal = castNs.validate("test")
    if ok or not errKind then
        return ok
    elseif errKind == CEK.MIN then
        ---@cast errVal string
        local r = castNeedsStr(errVal)
--                              ^ hover: (local) errVal: string
    elseif errKind == CEK.MAX then
        ---@cast errVal number
        local r = errVal + 1
--                 ^ hover: (local) errVal: number
    else
        error("bad")
    end
end

---@return (true)
---|       (false, CastErrKind errKind, string errVal)
---|       (false, CastErrKind errKind, number errVal)
function castNs.validate(item)
    return true
end

-- ── Method hover on if-condition with @cast in then-branch (regression) ─────
-- The receiver's later cast version (and the merged version after the if) must
-- not leak into the receiver's type at the if-condition position. Otherwise
-- hover/completion at the method-name resolves the receiver to a union and
-- shows duplicate signatures from every parent class in the union members'
-- inheritance chains.

---@class CastIsaBase
---@field IsKind fun(self): boolean

---@class CastIsaTask: CastIsaBase

---@class CastIsaSub: CastIsaTask

---@type CastIsaTask
local castIsaTask

if castIsaTask:IsKind() then
--             ^ comp: IsKind
--                ^ hover: (method) function CastIsaTask:IsKind()\n-> boolean
    ---@cast castIsaTask CastIsaSub
    print(castIsaTask)
end

-- ── @cast with intervening plain comment ──────────────────────────────────────
-- A regular `--` comment between the @cast and the target statement should not
-- block the cast from being applied.

local castPlainVal = "hello"
---@cast castPlainVal string?
-- This is a regular comment
if castPlainVal then
    print(castPlainVal)
--        ^ hover: (local) castPlainVal: string
end

-- ── @cast with trailing blank line (applied) ─────────────────────────────────
-- A @cast immediately after a statement, followed by a blank line, should still
-- apply (the cast is trailing trivia of the defining statement).

---@return number
---@return number
---@return number
---@return number
local function castTrailingReturns()
    return 1, 2, 3, 4
end
local castTrA, castTrB, castTrC, castTrD = castTrailingReturns()
---@cast castTrD number?

if castTrD then
    print(castTrD)
--        ^ hover: (local) castTrD: number
end

-- ── @cast multiple trailing casts with blank line ────────────────────────────
-- Multiple @cast lines after the same statement, followed by a blank line,
-- should all apply in source order.

local castMuA, castMuB, castMuC = castTrailingReturns()
---@cast castMuA +nil
---@cast castMuC string

print(castMuA)
--    ^ hover: (local) castMuA: number?
print(castMuC)
--    ^ hover: (local) castMuC: string

-- ── @cast blocked by blank line + comment ────────────────────────────────────
-- A @cast followed by a blank line AND a plain comment does not apply — the
-- comment after the blank line signals a section boundary.

local castBlankVal = "hello"
---@cast castBlankVal string?

-- unrelated section header
print(castBlankVal)
--    ^ hover: (local) castBlankVal: string = "hello"
