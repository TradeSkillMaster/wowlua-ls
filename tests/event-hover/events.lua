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
---| "BATCH_FUN_ALIAS" -> prepareFunc: PrepareFunc
---| "BATCH_FUN_INLINE" -> onDone: fun(ok: boolean): number

-- Function-typed payload via an @alias: the payload param keeps the alias's
-- fun(...) signature on hover instead of decaying to the bare word "function".
---@alias PrepareFunc fun(self: table, button: table)

-- Alias referencing an event type name — tests that event type aliases
-- (e.g. WowEvent → string) are resolved before dependent aliases.
---@alias AnyGameEvent WowEvent

---@class IteratorObject<F>

---@class EventFrame
local EventFrame = {}

---@param eventName WowEvent
function EventFrame:RegisterEvent(eventName) end

---@param eventName CustomEvent
function EventFrame:RegisterCustomEvent(eventName) end

---@param name string
function EventFrame:SetName(name) end

-- Generic register-by-name: the callback's varargs are typed from the
-- specific event literal bound to E.
---@generic E: ActionEvent
---@param event E
---@param callback fun(...params<E>)
function RegisterAction(event, callback) end

-- Colon-syntax method variant (self_offset = 1).
---@generic E: ActionEvent
---@param event E
---@param callback fun(...params<E>)
function EventFrame:OnAction(event, callback) end

-- Register-by-name constrained to BatchAction, used to exercise function-typed
-- payload params (e.g. BATCH_FUN_ALIAS -> prepareFunc: PrepareFunc) through the
-- inline-callback narrowing path.
---@generic E: BatchAction
---@param event E
---@param callback fun(...params<E>)
function RegisterFunPayload(event, callback) end

-- Callback annotation with a named param before ...params<E> (vararg_pos=1).
---@generic E: ActionEvent
---@param event E
---@param callback fun(label: string, ...params<E>)
function RegisterLabeled(event, callback) end

-- Callback annotation whose leading param is typed as the event type itself
-- (`fun(event: ActionEvent, ...params<E>)`), modelling the AceEvent handler shape
-- `function(event, ...)`: the event-typed param decays to string, and the payload
-- params/varargs after it map to the bound event's payload.
---@generic E: ActionEvent
---@param event E
---@param callback fun(event: ActionEvent, ...params<E>)
function RegisterEventShaped(event, callback) end

---@overload fun(self: EventFrame, script: "OnEvent", handler: fun(self: EventFrame, event: WowEvent, ...params<WowEvent>))
---@overload fun(self: EventFrame, script: "OnUpdate", handler: fun(self: EventFrame, elapsed: number))
---@param scriptType string
---@param handler function|nil
function EventFrame:SetScript(scriptType, handler) end
