---@diagnostic disable: create-global, unused-local, unused-function, shadowed-local
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

-- A function body's write makes no table of a global assigned a call: other
-- files read the call's type, with the field.
MAIN_FRAME = CreateFrame("Frame")
local function AddChild()
    MAIN_FRAME.child = CreateFrame("Button")
end

-- A saved variable initialized from another: the entry the scan can't type
-- (`SAVED_LEGACY.last`) reads here as this file types it, so a field written it
-- and a record keeps the record.
SelectionPanel = {}
SelectionPanel.trees = { { id = 1, name = "first" } }
SelectionPanel.selected = nil

function SelectionPanel:Initialize()
    SAVED_LEGACY = SAVED_LEGACY or {}
    SAVED_SELECTION = SAVED_SELECTION or { last = SAVED_LEGACY.last }
    self.saved = SAVED_SELECTION
end

function SelectionPanel:Restore()
    self.selected = self.saved and self.saved.last or nil
    for _, tree in ipairs(self.trees) do
        self.selected = tree
    end
end

function SelectionPanel:SelectedID()
    if self.selected then return self.selected.id end
    --                                         ^ hover: (field) id: number
end

-- A local of the same name, defaulted to a constructor too, is another table: its
-- entries stay off the global.
SAVED_LAYOUT = SAVED_LAYOUT or { width = 100 }

local function MigrateLayout(old)
    local SAVED_LAYOUT = old
    SAVED_LAYOUT = SAVED_LAYOUT or { width = 5, migrated = function() return true end }
    return SAVED_LAYOUT
end

local function LayoutMigrated()
    return SAVED_LAYOUT.migrated, MigrateLayout
    --                  ^ hover: <missing>  def: None
end
