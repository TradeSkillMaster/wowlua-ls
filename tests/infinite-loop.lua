-- Test: `while true` / `repeat until false` with only branching returns produces
-- a confident (non-nilable) return type, and suppresses `missing-return`.

---@diagnostic disable: empty-block, unused-local, unused-function, need-check-nil

-- ── Positive case: while true, every exit is `return <string>` ─────────────
local function f1()
    while true do
        if true then
            return "a"
        end
        if false then
            return "b"
        end
    end
end

local x = f1()
--    ^ hover: (global) x: string

-- ── Nested break doesn't escape the outer loop ────────────────────────────
local function f2()
    while true do
        while true do break end
        for _ = 1, 10 do break end
        if true then
            return "a"
        end
    end
end

local y = f2()
--    ^ hover: (global) y: string

-- ── Top-level break DOES escape: fall-through is reachable ────────────────
local function f3()
    while true do
        if true then
            break
        end
        if false then
            return "a"
        end
    end
end

local z = f3()
--    ^ hover: (global) z: string | nil

-- ── repeat ... until false ────────────────────────────────────────────────
local function f4()
    repeat
        if true then
            return "a"
        end
    until false
end

local w = f4()
--    ^ hover: (global) w: string

-- ── @return-annotated infinite-loop function should NOT get missing-return ─
---@return string name
local function f5()
-- ^ diag: none
    while true do
        if true then
            return "ok"
        end
    end
end

local nm = f5()
--    ^ hover: (global) nm: string

-- ── break inside a nested function body doesn't escape ────────────────────
local function f6()
    while true do
        local cb = function()
            return 1
        end
        cb()
        if true then
            return "a"
        end
    end
end

local v = f6()
--    ^ hover: (global) v: string
