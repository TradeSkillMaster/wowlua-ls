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
--                              ^ hover: (param) self: EventFrame
--                                       ^ hover: (param) event: WowEvent
--                                              ^ hint: : params<WowEvent>
    local s = self
--        ^ hover: (local) s: EventFrame
    local e = event
--        ^ hover: (local) e: WowEvent
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

-- ── Alias referencing event type (regression: alias ordering) ──

---@param ev AnyGameEvent
local function handleEvent(ev) end
local _h = handleEvent
--    ^ hover: (local) function _h(ev: AnyGameEvent)  diag: none

-- ── Standalone function with params<EventType> (no callback/SetScript) ──

---@param action ActionEvent
---@param ... params<ActionEvent>
local function handleAction(action, ...)
--                          ^ hover: (param) action: ActionEvent  diag: none
    if action == "DO_PROCESS" then
        local success, canRetry = ...
        local s = success
--            ^ hover: (local) s: boolean
        local c = canRetry
--            ^ hover: (local) c: boolean
    end
    if action == "DO_SKIP" then
        local test = ...
--            ^ hover: (local) test: ?
    end
    if action == "DO_RESET" then
        local test = ...
--            ^ hover: (local) test: ?
    end
end

-- Event hover in standalone function equality comparison
handleAction("DO_PROCESS")
--            ^ hover: (event) DO_PROCESS → success: boolean, canRetry: boolean

handleAction("DO_SKIP")
--            ^ hover: (event) DO_SKIP

-- ── Generic function with params<T> where T: EventType ──

---@generic A: ActionEvent
---@param action A
---@param ... params<A>
local function handleActionGeneric(action, ...)
--                                 ^ hover: (param) action: A  diag: none
    if action == "DO_PROCESS" then
        local success, canRetry = ...
        local s = success
--            ^ hover: (local) s: boolean
        local c = canRetry
--            ^ hover: (local) c: boolean
    end
    if action == "DO_SKIP" then
        local test = ...
--            ^ hover: (local) test: ?
    end
end
handleActionGeneric("DO_PROCESS", true, false)
-- NOTE: call-site event hover not yet supported for generic functions
-- (the non-generic handleAction path at line 135 covers this)

-- No doc-func-no-function on @param inside @event blocks (regression test)
---@event ActionEvent "DO_REFRESH"
---@param count number
-- ^ diag: none

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

-- ── Batch event declarations via ---| ──

---@param action BatchAction
local function handleBatchHover(action) end

handleBatchHover("BATCH_START")
--                ^ hover: (event) BATCH_START → scanType: string, scanContext: table  def: external

handleBatchHover("BATCH_COMPLETED")
--                ^ hover: (event) BATCH_COMPLETED  def: external

handleBatchHover("BATCH_RESULT")
--                ^ hover: (event) BATCH_RESULT → success: boolean, canRetry: boolean  def: external

handleBatchHover("BATCH_OPTIONAL")
--                ^ hover: (event) BATCH_OPTIONAL → name: string, count?: number  def: external

-- Completions for batch events
handleBatchHover("")
--                ^ comp: BATCH_COMPLETED, BATCH_GENERIC_PARAM, BATCH_OPTIONAL, BATCH_RESULT, BATCH_START

-- params<BatchAction> narrowing
---@param action BatchAction
---@param ... params<BatchAction>
local function handleBatchAction(action, ...)
    if action == "BATCH_START" then
        local scanType, scanContext = ...
        local s = scanType
--            ^ hover: (local) s: string
    end
    if action == "BATCH_RESULT" then
        local success, canRetry = ...
        local s = success
--            ^ hover: (local) s: boolean
        local c = canRetry
--            ^ hover: (local) c: boolean
    end
    if action == "BATCH_COMPLETED" then
        local test = ...
--            ^ hover: (local) test: ?
    end
end

-- ── Parameterized types in event payload narrowing ──

---@param action BatchAction
---@param ... params<BatchAction>
local function handleBatchParamTypes(action, ...)
    if action == "BATCH_GENERIC_PARAM" then
        local iter = ...
        local i = iter
--            ^ hover: (local) i: IteratorObject
-- (base class resolved; type args not substituted by resolve_annotation_type)
    end
end

-- ── Inline params on single @event ──

---@param ev InlineEvent
local function handleInlineHover(ev) end

handleInlineHover("INLINE_ONE")
--                 ^ hover: (event) INLINE_ONE → code: number  def: external

handleInlineHover("INLINE_TWO")
--                 ^ hover: (event) INLINE_TWO → x: string, y: boolean  def: external

handleInlineHover("INLINE_NONE")
--                 ^ hover: (event) INLINE_NONE  def: external

handleInlineHover("")
--                 ^ comp: INLINE_NONE, INLINE_ONE, INLINE_TWO

-- ── Inline callback varargs typed from a bound event-name generic ──

RegisterAction("DO_PROCESS", function(...)
    local first = ...
--                ^ hover: (varargs) ...: boolean, boolean
    local s = first
--        ^ hover: (local) s: boolean
end)

RegisterAction("DO_SKIP", function(...)
    local test = ...
--               ^ hover: (varargs) ...: ?
end)

-- Colon-syntax: self_offset shifts param_annotations index
---@type EventFrame
local ef = nil
ef:OnAction("DO_PROCESS", function(...)
    local a = ...
--            ^ hover: (varargs) ...: boolean, boolean
    local b = a
--        ^ hover: (local) b: boolean
end)
