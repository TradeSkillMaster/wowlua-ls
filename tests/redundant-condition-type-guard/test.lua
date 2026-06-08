-- Test: redundant-condition `type()` guard cases (requires stubs for `type`)
---@diagnostic disable: unused-local, unused-function, empty-block

-- `type(n) == "number"` where n is a number → always true
---@type number
local n
if type(n) == "number" then end
--    ^ diag: redundant-condition

-- `type(n) == "string"` where n is a number → always false
---@type number
local n2
if type(n2) == "string" then end
--    ^ diag: redundant-condition

-- `type(n) ~= "number"` where n is a number → always false
---@type number
local n3
if type(n3) ~= "number" then end
--    ^ diag: redundant-condition

-- Reversed operand order: `"number" == type(s)` where s is a string → false
---@type string
local s
if "number" == type(s) then end
--    ^ diag: redundant-condition

-- Mixed union → NOT flagged (could be either type)
---@type string|number
local u
if type(u) == "string" then end

-- Non-type-name literal: `type()` can only return one of the 8 Lua type names,
-- so comparing against "widget" is always false (resolved via the disjoint-type
-- path against the `type` stub's literal-union return).
---@type number
local n4
if type(n4) == "widget" then end
--    ^ diag: redundant-condition

-- ── Exit-else defensive guard suppression ────────────────────────────────────

local function _use(...) end

-- Last elseif condition is "always true" after narrowing a closed union via
-- type() guards, but is suppressed because the else block calls error().
---@param val number|string|table
local function checkTypeExitElse(val)
    if type(val) == "number" then
        _use(val)
    elseif type(val) == "string" then
        _use(val)
    elseif type(val) == "table" then
        -- suppressed: "always true" but else exits
        _use(val)
    else
        error("unexpected type")
    end
end
_use(checkTypeExitElse)

-- Same with exit-else return
---@param val number|string|table
local function checkTypeExitElseReturn(val)
    if type(val) == "number" then
        _use(val)
    elseif type(val) == "string" then
        _use(val)
    elseif type(val) == "table" then
        -- suppressed: else returns
        _use(val)
    else
        return
    end
end
_use(checkTypeExitElseReturn)

-- Still flag when there is no else block
---@param val number|string|table
local function checkTypeNoElse(val)
    if type(val) == "number" then
        _use(val)
    elseif type(val) == "string" then
        _use(val)
    elseif type(val) == "table" then
        --    ^ diag: redundant-condition
        _use(val)
    end
end
_use(checkTypeNoElse)

-- Still flag when the else block does not exit
---@param val number|string|table
local function checkTypeNonExitElse(val)
    if type(val) == "number" then
        _use(val)
    elseif type(val) == "string" then
        _use(val)
    elseif type(val) == "table" then
        --    ^ diag: redundant-condition
        _use(val)
    else
        _use("fallback")
    end
end
_use(checkTypeNonExitElse)
