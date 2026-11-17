-- Test: `@noreturn` — a call to a function tagged `@noreturn` never comes back,
-- so a block ending in one leaves the way a `return` does: a function ending in
-- one isn't `missing-return`, and an `if` branch ending in one narrows the code
-- after the `if` like an early `return`. `error` is one through its stub tag.

---@diagnostic disable: unused-local, unused-function, shadowed-local

-- ── `error` (tagged in its stub) ───────────────────────────────────────────

---@return number
local function viaError(x)
    if x then return 1 end
    error("no value")
end

---@param s string?
local function narrowViaError(s)
    if not s then error("no value") end
    return s
    --     ^ hover: (param) s: string
end

-- ── A local helper ─────────────────────────────────────────────────────────

---@param msg string
---@noreturn
local function Fail(msg)
    error("failed: " .. msg)
end

---@return number
local function viaHelper(x)
    if x then return 1 end
    Fail("no value")
end

---@param s string?
local function narrowViaHelper(s)
    if not s then Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

-- Every branch of an if/else ending in one, or a return.
---@return number
local function allBranches(x)
    if x == 1 then
        return 1
    elseif x == 2 then
        Fail("two")
    else
        error("other")
    end
end

-- A type guard whose failing branch ends in one.
---@param v string|number
local function typeGuard(v)
    if type(v) ~= "string" then
        Fail("not a string")
    end
    return v
    --     ^ hover: (param) v: string
end

-- A nil guard's then-block that assigns in one branch and leaves in the other.
---@param s string?
---@param fallback string?
local function nestedEnsure(s, fallback)
    if not s then
        if fallback then
            s = fallback
        else
            Fail("no value")
        end
    end
    return s
    --     ^ hover: (param) s: string
end

-- A helper the function declares itself.
---@return number
local function ownHelper(x)
    ---@noreturn
    local function bail()
        error("no value")
    end
    if x then return 1 end
    bail()
end

---@param s string?
local function narrowViaOwnHelper(s)
    ---@noreturn
    local function bail()
        error("no value")
    end
    if not s then bail() end
    return s
    --     ^ hover: (param) s: string
end

-- ── A table's function, and a method ───────────────────────────────────────

local Util = {}

---@param msg string
---@noreturn
function Util.Fail(msg)
    error(msg)
end

---@param s string?
local function narrowViaField(s)
    if not s then Util.Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@return number
local function viaField(x)
    if x then return 1 end
    Util.Fail("no value")
end

---@class NoReturnChecker
local Checker = {}

---@param msg string
---@noreturn
function Checker:Fail(msg)
    error(msg)
end

---@param s string?
function Checker:Require(s)
    if not s then self:Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

---@return number
function Checker:Get()
    self:Fail("not implemented")
end

-- ── Known only once types resolve ──────────────────────────────────────────
-- `missing-return` is decided after resolve, so it sees what the build can't
-- yet: a helper defined further down the file, a forward-declared local
-- assigned below. Narrowing happens during the build, so it needs the helper
-- defined above the `if` (a receiver a call returns is typed from the call's
-- `@return`).

---@return NoReturnChecker
local function GetChecker()
    return Checker
end

---@return number
local function viaReturnedReceiver()
    local checker = GetChecker()
    checker:Fail("no value")
end

---@param s string?
local function narrowViaReturnedReceiver(s)
    local checker = GetChecker()
    if not s then checker:Fail("no value") end
    return s
    --     ^ hover: (param) s: string
end

local Later = {}

---@return number
local function viaLaterHelper(x)
    if x then return 1 end
    Later.Fail("no value")
end

-- Defined below the `if`: no narrowing.
---@param s string?
local function narrowViaLaterHelper(s)
    if not s then Later.Fail("no value") end
    return s
    --     ^ hover: (param) s: string?
end

---@param msg string
---@noreturn
function Later.Fail(msg)
    error(msg)
end

local declaredFail

---@return number
local function viaForwardDeclared(x)
    if x then return 1 end
    declaredFail("no value")
end

---@param msg string
---@noreturn
declaredFail = function(msg)
    error(msg)
end

-- `error` read through a localized `_G`.
do
    local _G = _G
    local error = _G.error

    ---@return number
    local function viaAliasedError(x)
        if x then return 1 end
        error("no value")
    end

    ---@param s string?
    local function narrowViaAliasedError(s)
        if not s then error("no value") end
        return s
        --     ^ hover: (param) s: string
    end
end

-- ── Not exits ──────────────────────────────────────────────────────────────

-- A helper that always errors but isn't tagged returns as far as its callers know.
---@param msg string
local function untaggedFail(msg)
    error(msg)
end

---@return number
local function viaUntagged(x)
-- ^ diag: missing-return
    if x then return 1 end
    untaggedFail("no value")
end

---@param s string?
local function narrowViaUntagged(s)
    if not s then untaggedFail("no value") end
    return s
    --     ^ hover: (param) s: string?
end

-- A local the branch declares before the call is what the call runs, not the
-- helper of the same name.
---@param s string?
local function shadowedInBranch(s)
    if not s then
        local Fail = print
        Fail("no value")
    end
    return s
    --     ^ hover: (param) s: string?
end

---@return number
local function shadowedAtEnd(x)
-- ^ diag: missing-return
    if x then
        return 1
    else
        local Fail = print
        Fail("no value")
    end
end

-- An `if` without an `else` can fall through even when its branch leaves.
---@return number
local function ifWithoutElse(x)
-- ^ diag: missing-return
    if not x then Fail("no value") end
end

-- The tag outside a function is reported like any function-level tag.
---@noreturn
-- ^ diag: doc-func-no-function
local notAFunction = 1

-- A function named `error` that isn't the stub's: only the tag counts, not the name.
---@param msg string
local function error(msg)
    print(msg)
end

---@return number
local function viaLocalError(x)
-- ^ diag: missing-return
    if x then return 1 end
    error("not really")
end

---@param s string?
local function narrowViaLocalError(s)
    if not s then error("not really") end
    return s
    --     ^ hover: (param) s: string?
end
