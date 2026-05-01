---@type EventFrame
local f = nil

f:RegisterEvent("ENCOUNTER_END")
--                ^ hover: (event) ENCOUNTER_END(\n  encounterID: number,\n  encounterName: string,\n  difficultyID: number,\n  groupSize: number,\n  success: number\n)

f:RegisterEvent("ADDON_LOADED")
--                ^ hover: (event) ADDON_LOADED(addOnName: string)

f:RegisterEvent("PLAYER_LOGIN")
--                ^ hover: (event) PLAYER_LOGIN

f:RegisterCustomEvent("MY_ADDON_READY")
--                      ^ hover: (event) MY_ADDON_READY(version: string, isDebug: boolean)

---@param frame EventFrame
---@param eventName WowEvent
local function staticRegister(frame, eventName) end

staticRegister(f, "ENCOUNTER_END")
--                  ^ hover: (event) ENCOUNTER_END(\n  encounterID: number,\n  encounterName: string,\n  difficultyID: number,\n  groupSize: number,\n  success: number\n)

f:RegisterEvent('ADDON_LOADED')
--                ^ hover: (event) ADDON_LOADED(addOnName: string)

f:RegisterEvent("NONEXISTENT_EVENT")
--                ^ hover: <missing>

f:SetName("hello")
--          ^ hover: <missing>
