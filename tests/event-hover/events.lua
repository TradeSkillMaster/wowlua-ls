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

---@event ActionEvent "DO_PROCESS"
---@param success boolean
---@param canRetry boolean

---@event ActionEvent "DO_SKIP"

---@event ActionEvent "DO_RESET"

-- Inline params on single-event form
---@event InlineEvent "INLINE_ONE" -> code: number
---@event InlineEvent "INLINE_TWO" -> x: string, y: boolean
---@event InlineEvent "INLINE_NONE"

-- Batch event declarations via ---|
---@event BatchAction
---| "BATCH_START" -> scanType: string, scanContext: table
---| "BATCH_COMPLETED"
---| "BATCH_RESULT" -> success: boolean, canRetry: boolean
---| "BATCH_OPTIONAL" -> name: string, count?: number
---| "BATCH_GENERIC_PARAM" -> iter: IteratorObject<fun(): number, string>

-- Alias referencing an event type name — tests that event type aliases
-- (e.g. WowEvent → string) are resolved before dependent aliases.
---@alias AnyGameEvent WowEvent

---@class EventFrame
local EventFrame = {}

---@param eventName WowEvent
function EventFrame:RegisterEvent(eventName) end

---@param eventName CustomEvent
function EventFrame:RegisterCustomEvent(eventName) end

---@param name string
function EventFrame:SetName(name) end

---@overload fun(self: EventFrame, script: "OnEvent", handler: fun(self: EventFrame, event: WowEvent, ...params<WowEvent>))
---@overload fun(self: EventFrame, script: "OnUpdate", handler: fun(self: EventFrame, elapsed: number))
---@param scriptType string
---@param handler function|nil
function EventFrame:SetScript(scriptType, handler) end
