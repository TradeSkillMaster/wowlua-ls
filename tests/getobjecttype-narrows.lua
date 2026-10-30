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

-- ── Guarded read on a backward-inferred param ────────────────────────────────
-- `widget` is inferred `LocalWidget` from the call, but the guard can't narrow
-- it before then (a `GetObjectType` of its own beside the API's), so the read
-- under it is of the argument: `LocalWidget` lacks `SetLabel`, and the param
-- stays untyped rather than flagging it.

---@class LocalWidget
local LocalWidget = {}
---@returns-class-name
---@return string
function LocalWidget:GetObjectType() return "LocalWidget" end
function LocalWidget:Hide() end

---@class LocalLabel : LocalWidget
local LocalLabel = {}
function LocalLabel:SetLabel(text) end

---@param w LocalWidget
local function hideLocalWidget(w) w:Hide() end

local function test_inferred_guarded(widget)
--                                   ^ hover: (param) widget: ?
    hideLocalWidget(widget)
    if widget:GetObjectType() == "LocalLabel" then
        widget:SetLabel("x")
    end
end

-- ── Field-chain receiver ─────────────────────────────────────────────────────
-- The field the method is called on is narrowed, not the variable it's read
-- through.

---@class RegionHolder
---@field region Region
local RegionHolder = {}

function RegionHolder:test_field_receiver()
    if self.region:GetObjectType() == "FontString" then
        local s = self.region:GetText()
        --    ^ hover: (local) s: string
        --                    ^ hover: (method) function FontString:GetText()
    end
end

-- ── Function read off an untyped table ───────────────────────────────────────
-- `util.GetObjectType` is whatever `util` holds; the widget method only shares
-- its name, so nothing is narrowed.

---@class CountedRegionItem
---@field count number

---@param item CountedRegionItem
local function test_untyped_table_call(util, item)
    if util.GetObjectType(item) == "Button" then
        local c = item.count
        --    ^ hover: (local) c: number
    end
end
