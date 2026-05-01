---@meta _

---[Documentation](https://warcraft.wiki.gg/wiki/ENCOUNTER_END)
---@event WowEvent "ENCOUNTER_END"
---@param encounterID number
---@param encounterName string
---@param difficultyID number
---@param groupSize number
---@param success number

---[Documentation](https://warcraft.wiki.gg/wiki/ADDON_LOADED)
---@event WowEvent "ADDON_LOADED"
---@param addOnName string

---[Documentation](https://warcraft.wiki.gg/wiki/PLAYER_LOGIN)
---@event WowEvent "PLAYER_LOGIN"

---Custom addon event
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
