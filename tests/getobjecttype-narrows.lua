---@diagnostic disable: unused-function, unused-local
-- Tests for `recv:GetObjectType() == "ClassName"` narrowing via the
-- `@returns-class-name` annotation on FrameScriptObject:GetObjectType.
-- Analogous to tests/isobjecttype-narrows.lua, but the guard is a return-value
-- equality comparison rather than a boolean `IsObjectType` call.

-- ── Then-branch narrowing ────────────────────────────────────────────────────

---@param region Region
local function test_then_branch(region)
    if region:GetObjectType() == "FontString" then
        local x = region
        --    ^ hover: (local) x: FontString
    end
end

-- ── Guarded field access resolves (the motivating false positive) ────────────

---@param region Region
local function test_guarded_access(region)
    if region:GetObjectType() == "FontString" then
        -- GetText is on FontString, not the base Region — must NOT be undefined-field.
        local s = region:GetText()
        --    ^ hover: (local) s: string
    end
end

-- ── Reversed operands ────────────────────────────────────────────────────────

---@param region Region
local function test_reversed(region)
    if "EditBox" == region:GetObjectType() then
        local x = region
        --    ^ hover: (local) x: EditBox
    end
end

-- ── Early-exit narrowing (`~=` then return) ──────────────────────────────────

---@param region Region
local function test_early_exit(region)
    if region:GetObjectType() ~= "Button" then return end
    local x = region
    --    ^ hover: (local) x: Button
end

-- ── And-chain narrowing ──────────────────────────────────────────────────────

---@param region Region
local function test_and_chain(region)
    if region:IsShown() and region:GetObjectType() == "Button" then
        local x = region
        --    ^ hover: (local) x: Button
    end
end

-- ── assert() narrowing ───────────────────────────────────────────────────────

---@param region Region
local function test_assert(region)
    assert(region:GetObjectType() == "Slider")
    local x = region
    --    ^ hover: (local) x: Slider
end

-- ── Return type match after narrowing ────────────────────────────────────────

---@param region Region
---@return FontString|nil
local function test_return_narrowed(region)
    if region:GetObjectType() == "FontString" then
        return region
    end
end

-- ── Param type match after narrowing ─────────────────────────────────────────

---@param fs FontString
local function useFontString(fs) end

---@param region Region
local function test_param_narrowed(region)
    if region:GetObjectType() == "FontString" then
        useFontString(region)
    end
end

-- ── Else-branch should NOT narrow ────────────────────────────────────────────

---@param region Region
local function test_else_no_narrow(region)
    if region:GetObjectType() == "FontString" then
        local x = region
        --    ^ hover: (local) x: FontString
    else
        local y = region
        --    ^ hover: (local) y: Region
    end
end

-- ── Non-literal comparison: no narrowing ─────────────────────────────────────

---@param region Region
---@param typeName string
local function test_non_literal(region, typeName)
    if region:GetObjectType() == typeName then
        local x = region
        --    ^ hover: (local) x: Region
    end
end

-- ── Unknown class name: graceful degradation ─────────────────────────────────

---@param region Region
local function test_unknown_class(region)
    if region:GetObjectType() == "NotARealClass" then
        local x = region
        --    ^ hover: (local) x: Region
    end
end

-- ── Without the guard, the subclass field is genuinely undefined ─────────────

---@param region Region
local function test_unguarded_is_error(region)
    local s = region:GetText()
    --               ^ diag: undefined-field
end

-- ── Local @returns-class-name method, trailing text ignored ──────────────────
-- The tag takes no arguments; any trailing tokens must be ignored, NOT reparsed
-- as a `@return` type (which previously emitted a bogus `undefined-doc-name` for
-- the `s-class-name` fragment). The exhaustive diagnostic check guards that here.

---@class LocalBase
local LocalBase = {}

---@class LocalDerived : LocalBase
local LocalDerived = {}

---@return number
function LocalDerived:value() return 0 end

---@returns-class-name with trailing tokens that must be ignored
---@return string
function LocalBase:kind() return "" end

---@param t LocalBase
local function test_local_trailing(t)
    if t:kind() == "LocalDerived" then
        -- value() is on LocalDerived only — resolves iff t narrowed, proving the
        -- @returns-class-name flag was set despite the trailing text.
        local v = t:value()
        --    ^ hover: (local) v: number
    end
end
