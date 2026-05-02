---@type EventFrame
local f = nil

f:RegisterEvent("ENCOUNTER_END")
--                ^ hover: (event) ENCOUNTER_END →\n  encounterID: number,\n  encounterName: string,\n  difficultyID: number,\n  groupSize: number,\n  success: number  doc: warcraft.wiki.gg/wiki/ENCOUNTER_END  def: external

f:RegisterEvent("ADDON_LOADED")
--                ^ hover: (event) ADDON_LOADED → addOnName: string  doc: warcraft.wiki.gg/wiki/ADDON_LOADED  def: external

f:RegisterEvent("PLAYER_LOGIN")
--                ^ hover: (event) PLAYER_LOGIN  doc: warcraft.wiki.gg/wiki/PLAYER_LOGIN  def: external

f:RegisterCustomEvent("MY_ADDON_READY")
--                      ^ hover: (event) MY_ADDON_READY → version: string, isDebug: boolean  doc: Custom addon event  def: external

---@param frame EventFrame
---@param eventName WowEvent
local function staticRegister(frame, eventName) end

staticRegister(f, "ENCOUNTER_END")
--                  ^ hover: (event) ENCOUNTER_END →\n  encounterID: number,\n  encounterName: string,\n  difficultyID: number,\n  groupSize: number,\n  success: number  doc: warcraft.wiki.gg/wiki/ENCOUNTER_END  def: external

f:RegisterEvent('ADDON_LOADED')
--                ^ hover: (event) ADDON_LOADED → addOnName: string  doc: warcraft.wiki.gg/wiki/ADDON_LOADED  def: external

f:RegisterEvent("NONEXISTENT_EVENT")
--                ^ hover: <missing>  def: None

f:SetName("hello")
--          ^ hover: <missing>

-- ── SetScript handler contextual typing ──

f:SetScript("OnEvent", function(self, event, ...)
    local s = self
--        ^ hover: (local) s: EventFrame
    local e = event
--        ^ hover: (local) e: string
end)

f:SetScript("OnUpdate", function(self, elapsed)
    local dt = elapsed
--        ^ hover: (local) dt: number
end)

-- ── Event-param narrowing: varargs get typed per-event ──

f:SetScript("OnEvent", function(self, event, ...)
    if event == "ENCOUNTER_END" then
        local encounterID, encounterName, difficultyID, groupSize, success = ...
        local id = encounterID
--            ^ hover: (local) id: number
        local name = encounterName
--            ^ hover: (local) name: string
    end
    if event == "ADDON_LOADED" then
        local addOnName = ...
        local n = addOnName
--            ^ hover: (local) n: string
    end
end)
