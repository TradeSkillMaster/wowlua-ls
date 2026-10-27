---@diagnostic disable: create-global, unused-local, unused-function
-- Saved variables an event handler initializes are those tables in other files
-- (saved_user.lua), and keep reading as this file writes them here.
local function InitializeSavedVariables()
    if SAVED_STATE == nil then
        SAVED_STATE = {
            Version = 2,
            Characters = {},
        }
    end
    SAVED_STATE.Region = GetCurrentRegion()
end

local function InitializeSettings()
    SAVED_SETTINGS = SAVED_SETTINGS or {}
    SAVED_SETTINGS.addons = SAVED_SETTINGS.addons or {}
    return SAVED_SETTINGS.addons
    --                    ^ hover: (field) addons: table
end

-- Written before the handler below initializes it.
local function UpdateKeys()
    local resetTime = GetTime()
    if not SAVED_KEYS.resetTime or resetTime ~= SAVED_KEYS.resetTime then
        --            ^ hover: (field) resetTime: number
        SAVED_KEYS.resetTime = resetTime
    end
end

CreateFrame("Frame"):SetScript("OnEvent", function()
    if not SAVED_KEYS then
        SAVED_KEYS = {
            keys = {},
        }
    end
    UpdateKeys()
end)

-- A function body's write makes no table of a global assigned a call.
MAIN_FRAME = CreateFrame("Frame")
local function AddChild()
    MAIN_FRAME.child = CreateFrame("Button")
end
