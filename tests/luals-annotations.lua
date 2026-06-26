---@diagnostic disable: unused-local, unused-function
-- LuaLS-compatible annotation forms that wowlua_ls now accepts silently:
--   1. comma-separated multi-return `@return T1, T2`
--   2. `[T, T]` tuple type syntax
--   3. `?T` prefix-optional shorthand
-- None of these should produce a diagnostic or hint.

local function _consume(...) end

-- ══════════════════════════════════════════════════════════════════════════
-- 1. Comma-separated `@return T1, T2` (LuaLS multi-return on one line)
-- ══════════════════════════════════════════════════════════════════════════

-- Two bare types
---@return string, boolean
local function two() return "x", true end
_consume(two)

two()
-- ^ sig: fun(): string, boolean

local a1, b1 = two()
local _ = a1
--        ^ hover: (local) a1: string
local _ = b1
--        ^ hover: (local) b1: boolean

-- Three bare types
---@return number, string, boolean
local function three() return 1, "x", true end
local x3, y3, z3 = three()
local _ = x3
--        ^ hover: (local) x3: number
local _ = z3
--        ^ hover: (local) z3: boolean

-- Array type as a segment (`string[]` keeps its `[]` together)
---@return string[], number
local function arrAndNum() return {}, 1 end
local arr, n = arrAndNum()
local _ = arr
--        ^ hover: (local) arr: string[]
local _ = n
--        ^ hover: (local) n: number

-- Per-segment names
---@return string text, number count
local function named() return "x", 1 end
local t, c = named()
local _ = t
--        ^ hover: (local) t: string
local _ = c
--        ^ hover: (local) c: number

-- Optional segment (`number?` mid-list)
---@return number?, string
local function optFirst() return nil, "x" end
local of1, of2 = optFirst()
local _ = of1
--        ^ hover: (local) of1: number?
local _ = of2
--        ^ hover: (local) of2: string

-- Trailing `# description` after the last type
---@return number, string # the pair
local function withDesc() return 1, "x" end
local wd1, wd2 = withDesc()
local _ = wd1
--        ^ hover: (local) wd1: number
local _ = wd2
--        ^ hover: (local) wd2: string

-- Returning exactly the annotated arity is balanced (no redundant-return-value,
-- no unbalanced-assignments).
---@return number, number
local function pair() return 1, 2 end
local pa, pb = pair()
_consume(pa, pb)

-- ── Disambiguation: a comma inside a DESCRIPTION is not a type separator ──

-- `<type> <name> <description-with-comma>` stays a SINGLE return. The second
-- "type" is description text, so there must be no undefined-doc-name and only
-- one return value.
---@return number red Red color, from 0 to 1
local function single1() return 1 end
local s1 = single1()
local _ = s1
--        ^ hover: (local) s1: number

-- Description with a comma after a backtick'd word also stays single.
---@return boolean ok `true` if set, `false` otherwise
local function single2() return true end
local s2 = single2()
local _ = s2
--        ^ hover: (local) s2: boolean

-- Parenthesized tuple form is unaffected (commas are inside `()`).
---@return (number a, string b)
local function tup() return 1, "x" end
local tu1, tu2 = tup()
local _ = tu1
--        ^ hover: (local) tu1: number
local _ = tu2
--        ^ hover: (local) tu2: string

-- ══════════════════════════════════════════════════════════════════════════
-- 2. `[T, T]` tuple type syntax (lowers to { [1]: T1, [2]: T2, ... })
-- ══════════════════════════════════════════════════════════════════════════

---@class Tup
---@field size [number, number]
---@field coords [number, number, number, number]
---@field mixed [string, number]

---@type Tup
local inst

local e1 = inst.size[1]
local _ = e1
--        ^ hover: (local) e1: number

local e2 = inst.coords[3]
local _ = e2
--        ^ hover: (local) e2: number

-- Distinct element types in a heterogeneous tuple
local m1 = inst.mixed[1]
local _ = m1
--        ^ hover: (local) m1: string
local m2 = inst.mixed[2]
local _ = m2
--        ^ hover: (local) m2: number

-- Tuple as a local `@type` and as a `@param`
---@type [string, number]
local pairLocal
local pl1 = pairLocal[1]
local _ = pl1
--        ^ hover: (local) pl1: string

---@param p [number, number]
local function takesTuple(p)
    return p[2]
end
_consume(takesTuple)

-- ══════════════════════════════════════════════════════════════════════════
-- 3. `?T` prefix-optional shorthand (treated as optional `T`)
-- ══════════════════════════════════════════════════════════════════════════

-- Prefix-optional param: calling without it is fine (no missing-parameter).
---@param x number
---@param y ?number
local function pre(x, y) return x end
pre(1)
-- ^ sig: fun(x: number, y: number?): number

-- The canonical suffix form still works and renders identically.
---@param x number
---@param y number?
local function suf(x, y) return x end
suf(1)
-- ^ sig: fun(x: number, y: number?): number

-- Name-optional form (`y?`) is also unaffected (renders as `y?: number`,
-- distinct from the type-optional `y: number?` above).
---@param x number
---@param y? number
local function nameOpt(x, y) return x end
nameOpt(1)
--     ^ sig: fun(x: number, y?: number): number

-- Prefix-optional `@type` is nilable.
---@type ?boolean
local maybeBool
local _ = maybeBool
--        ^ hover: (local) maybeBool: boolean?

-- Prefix-optional `@field`.
---@class HasOpt
---@field tag ?string

---@type HasOpt
local ho
local hotag = ho.tag
local _ = hotag
--        ^ hover: (local) hotag: string?

-- Prefix-optional return.
---@return ?number
local function preRet() return nil end
local pr = preRet()
local _ = pr
--        ^ hover: (local) pr: number?
