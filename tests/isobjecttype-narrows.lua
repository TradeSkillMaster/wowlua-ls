-- Tests for IsObjectType() narrowing via @type-narrows 0 1 on FrameScriptObject

-- ── Then-branch narrowing ────────────────────────────────────────────────────

---@param frame Frame
local function test_then_branch(frame)
    if frame:IsObjectType("Button") then
        local x = frame
        --    ^ hover: (local) x: Button
    end
end

-- ── Early-exit narrowing ─────────────────────────────────────────────────────

---@param frame Frame
local function test_early_exit(frame)
    if not frame:IsObjectType("Button") then return end
    local x = frame
    --    ^ hover: (local) x: Button
end

-- ── And-chain narrowing ──────────────────────────────────────────────────────

---@param frame Frame
local function test_and_chain(frame)
    if frame:IsVisible() and frame:IsObjectType("Button") then
        local x = frame
        --    ^ hover: (local) x: Button
    end
end

-- ── Return type match after narrowing ────────────────────────────────────────

---@param frame Frame
---@return Button|nil
local function test_return_narrowed(frame)
    if frame:IsObjectType("Button") then
        return frame
        -- ^ diag: none
    end
end

-- ── Param type match after narrowing ─────────────────────────────────────────

---@param btn Button
local function useButton(btn) end

---@param frame Frame
local function test_param_narrowed(frame)
    if frame:IsObjectType("Button") then
        useButton(frame)
        -- ^ diag: none
    end
end

-- ── Else-branch should NOT narrow ────────────────────────────────────────────

---@param frame Frame
local function test_else_no_narrow(frame)
    if frame:IsObjectType("Button") then
        local x = frame
        --    ^ hover: (local) x: Button
    else
        local y = frame
        --    ^ hover: (local) y: Frame
    end
end

-- ── Non-literal argument: no narrowing ───────────────────────────────────────

---@param frame Frame
---@param typeName string
local function test_non_literal(frame, typeName)
    if frame:IsObjectType(typeName) then
        local x = frame
        --    ^ hover: (local) x: Frame
    end
end

-- ── Unknown class name: graceful degradation ─────────────────────────────────

---@param frame Frame
local function test_unknown_class(frame)
    if frame:IsObjectType("NotARealClass") then
        local x = frame
        --    ^ hover: (local) x: Frame
    end
end

-- ── Non-Button subclass (EditBox) ────────────────────────────────────────────

---@param frame Frame
local function test_editbox_narrow(frame)
    if frame:IsObjectType("EditBox") then
        local x = frame
        --    ^ hover: (local) x: EditBox
    end
end
