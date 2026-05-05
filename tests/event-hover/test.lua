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

-- ── Event name completions ──

f:RegisterEvent("")
--               ^ comp: ADDON_LOADED, ENCOUNTER_END, PLAYER_LOGIN

f:RegisterCustomEvent("")
--                     ^ comp: MY_ADDON_READY

staticRegister(f, "")
--                 ^ comp: ADDON_LOADED, ENCOUNTER_END, PLAYER_LOGIN

-- ── SetScript handler contextual typing ──

f:SetScript("OnEvent", function(self, event, ...)
--                                              ^ hint: : params<WowEvent>
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

-- ── Varargs hover: event-narrowed scope ──

f:SetScript("OnEvent", function(self, event, ...)
    if event == "ADDON_LOADED" then
        local test = ...
--                   ^ hover: (varargs) ...: string
    end
    if event == "ENCOUNTER_END" then
        local test = ...
--                   ^ hover: (varargs) ...: number, string, number, number, number
    end
end)

-- ── Varargs hover: parameter declaration ──

---@param ... number
local function varargFunc(...)
--                        ^ hover: (param) ...: number
--                           ^ hint: none
    local test = ...
--               ^ hover: (varargs) ...: number
end

local function plainVararg(...)
--                         ^ hover: (param) ...
end

-- ── Event string hover in equality comparison ──

f:SetScript("OnEvent", function(self, event, ...)
    if event == "ADDON_LOADED" then
--               ^ hover: (event) ADDON_LOADED → addOnName: string  doc: warcraft.wiki.gg/wiki/ADDON_LOADED  def: external
    end
    if event == "ENCOUNTER_END" then
--               ^ hover: (event) ENCOUNTER_END →\n  encounterID: number,\n  encounterName: string,\n  difficultyID: number,\n  groupSize: number,\n  success: number  doc: warcraft.wiki.gg/wiki/ENCOUNTER_END  def: external
    end
    if event ~= "PLAYER_LOGIN" then
--               ^ hover: (event) PLAYER_LOGIN  doc: warcraft.wiki.gg/wiki/PLAYER_LOGIN  def: external
    end
end)
