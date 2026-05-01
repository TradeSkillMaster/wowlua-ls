---@meta _

---@event WowEvent "ENCOUNTER_END"
---@param encounterID number
---@param encounterName string
---@param difficultyID number
---@param groupSize number
---@param success number

---@event WowEvent "ADDON_LOADED"
---@param addOnName string

---@event WowEvent "PLAYER_LOGIN"

---@event CustomEvent "MY_ADDON_READY"
---@param version string
---@param isDebug boolean

---@class EventFrame
local EventFrame = {}

---@param eventName WowEvent
function EventFrame:RegisterEvent(eventName) end

---@param eventName CustomEvent
function EventFrame:RegisterCustomEvent(eventName) end

---@param name string
function EventFrame:SetName(name) end
