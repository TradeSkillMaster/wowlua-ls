---@diagnostic disable: undefined-global
-- Test: unused-vararg diagnostic (default-disabled; enabled via .wowluarc.json)

local function _consume(...) return ... end

-- ── Fires: function declares `...` but never uses it ─────────────────────

local function unused(...)
--                    ^ diag: unused-vararg
    return 1
end
_consume(unused)

-- Global function form also fires.
function GlobalUnused(...)
--                   ^ diag: unused-vararg
    return "x"
end
_consume(GlobalUnused)

-- Colon method form fires too.
local tbl = {}
function tbl:method(...)
--                  ^ diag: unused-vararg
    return self
end
_consume(tbl)

-- ── No fire: `...` used in various ways ──────────────────────────────────

local function pack(...)
    return {...}
end
-- ^ diag: none
_consume(pack)

local function count(...)
    return select("#", ...)
end
-- ^ diag: none
_consume(count)

local function forward(...)
    return _consume(...)
end
-- ^ diag: none
_consume(forward)

local function first(...)
    local a = ...
    return a
end
-- ^ diag: none
_consume(first)

-- ── No fire: function has no `...` at all ────────────────────────────────

local function plain(a, b)
    return a + b
end
-- ^ diag: none
_consume(plain)

-- ── Nested inner functions don't count as using the outer `...` ──────────
-- An inner `function() ... end` has its own scope; `...` inside it would
-- be a parse-time error in Lua. We still expect the outer to fire even
-- though a nested inner function exists (it doesn't reference `...`).

local function outer_with_inner(...)
--                              ^ diag: unused-vararg
    local inner = function()
        return 42
    end
    return inner
end
_consume(outer_with_inner)

-- ── Suppression via `---@diagnostic disable` ─────────────────────────────

---@diagnostic disable-next-line: unused-vararg
local function suppressed(...)
    return 1
end
-- ^ diag: none
_consume(suppressed)
