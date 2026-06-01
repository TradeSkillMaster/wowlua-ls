---@diagnostic disable: undefined-global
local function _consume(...) end

---@param n number
local function _takeNum(n) _consume(n) end

---@param s string
local function _takeStr(s) _consume(s) end

-- ── Basic: two locals assigned in all branches of if/elseif (no else) ──────

---@param cond1 boolean
---@param cond2 boolean
local function basicCorrelation(cond1, cond2)
    local money = nil    ---@type number?
    local tradeType = nil ---@type string?
    if cond1 then
        tradeType = "buy"
        money = 100
    elseif cond2 then
        tradeType = "sell"
        money = 200
    end
    if not tradeType then return end
    -- After guard: both should be narrowed to non-nil
    local a = money
    --    ^ hover: (local) a: number
    _takeNum(money)
    -- ^ diag: none
    _takeStr(tradeType)
    -- ^ diag: none
end
_consume(basicCorrelation)

-- ── Early-exit guard: `if x == nil then return end` ─────────────────────

---@param cond1 boolean
---@param cond2 boolean
local function earlyExitNilEq(cond1, cond2)
    local amount = nil    ---@type number?
    local action = nil    ---@type string?
    if cond1 then
        action = "deposit"
        amount = 50
    elseif cond2 then
        action = "withdraw"
        amount = 75
    end
    if action == nil then return end
    local b = amount
    --    ^ hover: (local) b: number
    _takeNum(amount)
    -- ^ diag: none
end
_consume(earlyExitNilEq)

-- ── Reverse guard order: guard on the second variable ───────────────────

---@param cond1 boolean
---@param cond2 boolean
local function reverseGuard(cond1, cond2)
    local price = nil    ---@type number?
    local label = nil    ---@type string?
    if cond1 then
        label = "gold"
        price = 10
    elseif cond2 then
        label = "silver"
        price = 5
    end
    if not price then return end
    -- Guard on price should also narrow label
    local c = label
    --    ^ hover: (local) c: string
    _takeStr(label)
    -- ^ diag: none
end
_consume(reverseGuard)

-- ── Three correlated variables ──────────────────────────────────────────

---@param cond1 boolean
---@param cond2 boolean
local function threeVars(cond1, cond2)
    local x = nil ---@type number?
    local y = nil ---@type string?
    local z = nil ---@type boolean?
    if cond1 then
        x = 1
        y = "a"
        z = true
    elseif cond2 then
        x = 2
        y = "b"
        z = false
    end
    if not x then return end
    local d = y
    --    ^ hover: (local) d: string
    local e = z
    --    ^ hover: (local) e: boolean
end
_consume(threeVars)

-- ── Not correlated: one variable not assigned in all branches ───────────

---@param cond1 boolean
---@param cond2 boolean
local function notCorrelated(cond1, cond2)
    local alpha = nil ---@type number?
    local beta = nil  ---@type string?
    if cond1 then
        alpha = 1
        beta = "x"
    elseif cond2 then
        alpha = 2
        -- beta NOT assigned in this branch
    end
    if not alpha then return end
    -- beta should NOT be narrowed (not assigned in all branches)
    local f = beta
    --    ^ hover: (local) f: string?
end
_consume(notCorrelated)

-- ── Then-branch narrowing (if x then ... end) ──────────────────────────

---@param cond1 boolean
---@param cond2 boolean
local function thenBranch(cond1, cond2)
    local qty = nil  ---@type number?
    local side = nil ---@type string?
    if cond1 then
        qty = 10
        side = "left"
    elseif cond2 then
        qty = 20
        side = "right"
    end
    if qty then
        -- Inside then-branch: both should be narrowed
        local i = side
        --    ^ hover: (local) i: string
        _takeStr(side)
        -- ^ diag: none
    end
end
_consume(thenBranch)

-- ── Assert narrows correlated locals ────────────────────────────────────

---@param cond1 boolean
---@param cond2 boolean
local function assertNarrows(cond1, cond2)
    local count = nil ---@type number?
    local name = nil  ---@type string?
    if cond1 then
        count = 5
        name = "foo"
    elseif cond2 then
        count = 10
        name = "bar"
    end
    assert(count)
    local j = name
    --    ^ hover: (local) j: string
end
_consume(assertNarrows)

-- ── Single if branch (no elseif): still tracks correlation ──────────────

---@param cond boolean
local function singleBranch(cond)
    local m = nil ---@type number?
    local n = nil ---@type string?
    if cond then
        m = 42
        n = "hello"
    end
    if not m then return end
    local k = n
    --    ^ hover: (local) k: string
end
_consume(singleBranch)

-- ── Nested if/elseif chains: independent groups don't interfere ─────────

---@param c1 boolean
---@param c2 boolean
---@param c3 boolean
---@param c4 boolean
local function nestedChains(c1, c2, c3, c4)
    local a = nil ---@type number?
    local b = nil ---@type string?
    if c1 then
        a = 1
        b = "x"
        -- Inner chain creates its own group
        local p = nil ---@type number?
        local q = nil ---@type string?
        if c3 then
            p = 10
            q = "inner1"
        elseif c4 then
            p = 20
            q = "inner2"
        end
        if not p then return end
        local innerQ = q
        --    ^ hover: (local) innerQ: string
    elseif c2 then
        a = 2
        b = "y"
    end
    if not a then return end
    local outerB = b
    --    ^ hover: (local) outerB: string
end
_consume(nestedChains)

-- ── Reassignment after if/elseif breaks correlated narrowing ─────────────
-- Explicitly reassigning `b` to nil after the correlated branches removes it
-- from the group: narrowing `a` no longer implies `b` is non-nil.

---@param c1 boolean
---@param c2 boolean
local function reassignAfterChain(c1, c2)
    local a = nil ---@type number?
    local b = nil
    if c1 then
        a = 1
        b = "x"
    elseif c2 then
        a = 2
        b = "y"
    end
    b = nil
    if not a then return end
    -- b was reassigned after the branches: correlated narrowing does not apply
    local rb = b
    --    ^ hover: (local) rb: nil
end
_consume(reassignAfterChain)

-- ── Union dedup: separate `{}` literals in branches collapse to one `table` ──

---@param cond1 boolean
---@param cond2 boolean
local function dedupEmptyTableBranches(cond1, cond2)
    local t
    if cond1 then
        t = {}
    elseif cond2 then
        t = {}
    else
        t = {}
    end
    local u = t
    --    ^ hover: (local) u: table
end
_consume(dedupEmptyTableBranches)

-- ── Union dedup: `x = x or {}` across branches ─────────────────────────────

---@param cond1 boolean
---@param cond2 boolean
local function dedupOrAssign(cond1, cond2)
    local t = nil ---@type table?
    if cond1 then
        t = t or {}
    elseif cond2 then
        t = t or {}
    end
    -- Before the branch, t may be nil; in each branch, t = t or {} gives a table.
    -- After the merge, t should be `table | nil`, NOT `table | table | nil`.
    local u = t
    --    ^ hover: (local) u: table?
end
_consume(dedupOrAssign)

-- ── Narrowed field RHS in if/else branch merge ─────────────────────────────
-- When `location = private.field` inside `if private.field then`, the lowered
-- RHS is StripFalsy(FieldAccess(...)).  The branch merge must NOT treat this
-- as a synthetic narrowing version — it's a real assignment.

---@param x string
---@param y string
local function _doLog(x, y) _consume(x, y) end

local _priv = { overrideLocation = nil }

---@param loc string
local function _setOverride(loc) _priv.overrideLocation = loc end

---@return string?
local function _getLocation() return nil end

local function narrowedFieldBranchMerge()
    local location = nil
    if _priv.overrideLocation then
        location = _priv.overrideLocation
    else
        location = _getLocation()
        location = location and location or "?:?"
    end
    local r = location
    --    ^ hover: (local) r: string
    _doLog("INFO", location)
    -- ^ diag: none
end
_consume(narrowedFieldBranchMerge, _setOverride)

-- ── Reassignment inside narrowing scope resets nilability ────────────────
-- When a variable is reassigned inside `if x then`, the guard's nil-strip
-- must NOT persist onto the new value.

---@return boolean
---@return string?
local function _getResult() return true, "ok" end

local function reassignInsideGuard()
    local handled, otherPage = _getResult()
    local a = otherPage
    --    ^ hover: (local) a: string?
    if otherPage then
        local b = otherPage
        --    ^ hover: (local) b: string
        handled, otherPage = _getResult()
        local c = otherPage
        --    ^ hover: (local) c: string?
    end
end
_consume(reassignInsideGuard)

-- ── Multiple reassignments inside narrowing scope ────────────────────────
-- The override offset must be the FIRST reassignment, so all subsequent
-- references see the override.

local function multiReassignInsideGuard()
    local handled, otherPage = _getResult()
    if otherPage then
        local b = otherPage
        --    ^ hover: (local) b: string
        handled, otherPage = _getResult()
        local c = otherPage
        --    ^ hover: (local) c: string?
        handled, otherPage = _getResult()
        local d = otherPage
        --    ^ hover: (local) d: string?
    end
end
_consume(multiReassignInsideGuard)

-- ── Exiting middle branch in if/elseif/else: variable still non-nil ─────
-- When a middle elseif branch always exits (return/error), it should not
-- contribute nil to the merged type of variables assigned in all other branches.

---@param mode string
---@param fallback boolean
local function exitingMiddleBranch(mode, fallback)
    local reason = nil ---@type string?
    local value = nil  ---@type number?
    if mode == "buy" then
        reason = "buying"
        value = 100
    elseif mode == "invalid" then
        return nil
    elseif mode == "sell" then
        reason = "selling"
        value = 200
    else
        reason = "default"
        value = 0
    end
    local r = reason
    --    ^ hover: (local) r: string
    local v = value
    --    ^ hover: (local) v: number
    _takeStr(reason)
    -- ^ diag: none
    _takeNum(value)
    -- ^ diag: none
end
_consume(exitingMiddleBranch)

-- ── Exiting middle branch via error(): same as return ───────────────────

---@param mode string
local function exitingMiddleBranchError(mode)
    local label = nil ---@type string?
    if mode == "a" then
        label = "alpha"
    elseif mode == "b" then
        error("unsupported mode")
    else
        label = "other"
    end
    local l = label
    --    ^ hover: (local) l: string
end
_consume(exitingMiddleBranchError)

-- ── Manual @correlated annotation for locals ─────────────────────────────

---@type number?
local mCount = nil
---@type number?
local mOffset = nil
---@correlated mCount, mOffset
for _i = 1, 10 do
    if not mCount then
        mCount = _i
        mOffset = 0
    elseif mOffset < mCount then
--         ^ hover: (local) mOffset: number
        local x = mOffset + 1
        --    ^ hover: (local) x: number
        mOffset = mOffset + 1
    end
end
_consume(mCount, mOffset)

-- ── @correlated in loop with else-break: condition sees merged type ───────
-- When the guard is on a different variable (mCount2) than the one used in
-- the subsequent elseif condition (mOffset2), the correlated narrowing should
-- not produce `?` — the merge from prior loop iterations provides `number?`.

---@type number?
local mCount2 = nil
---@type number?
local mOffset2 = nil
---@correlated mCount2, mOffset2
for part in gmatch(str, "(%d*):") do
    part = tonumber(part)
    if not part then
        -- skip
    elseif not mCount2 then
        mCount2 = part or 0
        mOffset2 = 0
    elseif mOffset2 < mCount2 * 2 then
--         ^ hover: (local) mOffset2: number
        mOffset2 = mOffset2 + 1
    else
        break
    end
end
_consume(mCount2, mOffset2)

-- @correlated with unknown variable name warns
local knownVar = nil ---@type number?
---@correlated knownVar, unknownVar
-- ^ diag: malformed-annotation
local _sink = knownVar
_consume(_sink)

-- ── Complementary early-exit guards: a and not b / not a and b ──────────
-- After both branches exit, the remaining states are "both nil" or "both
-- non-nil". Then `~=` eliminates "both nil" (nil == nil is true in Lua).

---@param profit_a number?
---@param profit_b number?
local function complementaryExitGuards(profit_a, profit_b)
    if profit_a and not profit_b then
        return true
    elseif not profit_a and profit_b then
        return false
    end
    if profit_a ~= profit_b then
        return profit_a > profit_b
        --     ^ hover: (param) profit_a: number
    end
    return false
end
_consume(complementaryExitGuards)

-- Same pattern with elseif
---@param x number?
---@param y number?
local function complementaryExitElseif(x, y)
    if x and not y then
        return 1
    elseif not x and y then
        return 2
    end
    if x ~= y then
        local a = x
        --    ^ hover: (local) a: number
        local b = y
        --    ^ hover: (local) b: number
    end
end
_consume(complementaryExitElseif)

-- `==` in else-branch is equivalent to `~=` in then-branch
---@param m number?
---@param n number?
local function eqElseBranchNarrows(m, n)
    if m and not n then return
    elseif not m and n then return end
    if m == n then
        -- both could be nil here (nil == nil is true)
        local a = m
        --    ^ hover: (local) a: number?
    else
        -- m ~= n: eliminates both-nil → both non-nil
        local b = m
        --    ^ hover: (local) b: number
    end
end
_consume(eqElseBranchNarrows)

-- Non-complementary guards should NOT create correlation
---@param a number?
---@param b number?
local function nonComplementaryGuards(a, b)
    if a and not b then
        return
    elseif a and b then  -- not complementary (truthy={a,b}, falsy={})
        return
    end
    if a ~= b then
        local x = a
        --    ^ hover: (local) x: number?
    end
end
_consume(nonComplementaryGuards)

-- Truthiness guard after complementary exit also narrows the correlated sibling
---@param p number?
---@param q number?
local function truthinessGuardAfterComplementary(p, q)
    if p and not q then return
    elseif not p and q then return end
    if p then
        local a = q
        --    ^ hover: (local) a: number
    end
end
_consume(truthinessGuardAfterComplementary)

-- Three variables in complementary guards
---@param a number?
---@param b string?
---@param c boolean?
local function threeVarComplementary(a, b, c)
    if a and not b and not c then return
    elseif not a and b and c then return end
    if a ~= nil then
        local x = b
        --    ^ hover: (local) x: string
        local y = c
        --    ^ hover: (local) y: boolean
    end
end
_consume(threeVarComplementary)

-- ── Guard implications: `if A and not B then return` ⟹ A truthy implies B non-nil ──

---@param s string
---@return number?
local function _maxPrice(s) _consume(s) return 5 end

-- Basic: after the guard, narrowing the antecedent narrows the consequent non-nil
---@param itemString string?
local function guardImplicationBasic(itemString)
    local maxPrice = itemString and _maxPrice(itemString) or nil
    if itemString and not maxPrice then return true end
    if itemString then
        local x = maxPrice
        --    ^ hover: (local) x: number
        _consume(x)
    end
    return false
end
_consume(guardImplicationBasic)

-- Fires through an unrelated if/elseif chain (the elseif narrows the antecedent)
---@param itemString string?
local function guardImplicationElseif(itemString, flag)
    local maxPrice = itemString and _maxPrice(itemString) or nil
    if itemString and not maxPrice then return true end
    if flag then
        return true
    elseif itemString then
        local x = maxPrice
        --    ^ hover: (local) x: number
        _consume(x)
    end
    return false
end
_consume(guardImplicationElseif)

-- Reassigning the consequent after the guard invalidates the implication
---@param itemString string?
local function guardImplicationReassign(itemString)
    local maxPrice = itemString and _maxPrice(itemString) or nil
    if itemString and not maxPrice then return true end
    maxPrice = _maxPrice("other")
    if itemString then
        local x = maxPrice
        --    ^ hover: (local) x: number?
        _consume(x)
    end
    return false
end
_consume(guardImplicationReassign)

-- A guard nested inside a conditional branch must NOT leak past that branch
---@param itemString string?
local function guardImplicationNested(itemString, cond)
    local maxPrice = itemString and _maxPrice(itemString) or nil
    if cond then
        if itemString and not maxPrice then return true end
    end
    if itemString then
        local x = maxPrice
        --    ^ hover: (local) x: number?
        _consume(x)
    end
    return false
end
_consume(guardImplicationNested)

-- Multi-antecedent: both a AND b must be narrowed truthy before c is non-nil
---@param a string?
---@param b number?
---@param c boolean?
local function guardImplicationMultiAntecedent(a, b, c)
    if a and b and not c then return end
    if a then
        if b then
            local x = c
            --    ^ hover: (local) x: true
            _consume(x)
        end
    end
    return false
end
_consume(guardImplicationMultiAntecedent)

-- Multi-antecedent negative: only one of two antecedents narrowed → no narrowing
---@param a string?
---@param b number?
---@param c boolean?
local function guardImplicationPartialAntecedent(a, b, c)
    if a and b and not c then return end
    if a then
        local x = c
        --    ^ hover: (local) x: boolean?
        _consume(x)
    end
    return false
end
_consume(guardImplicationPartialAntecedent)
