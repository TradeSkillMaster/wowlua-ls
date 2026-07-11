---@meta _
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0)

--- The library object returned by `LibStub("AceEvent-3.0")`. It carries the
--- embeddable event/message methods (`RegisterEvent`, `RegisterMessage`, …) from the
--- `AceEvent` prototype, so both `LibStub("AceEvent-3.0"):Embed(target)` and the
--- common convention `---@class MyAddon : AceEvent-3.0` resolve those methods.
---@class AceEvent-3.0 : AceEvent
local AceEventLib = {}

--- Mix the AceEvent methods into `target`.
---@param target table
function AceEventLib:Embed(target) end

--- The embeddable AceEvent prototype: game-event and inter-addon message
--- registration. A registered handler is dispatched as a method on the object, so a
--- string `callback` (or, when it is omitted, the event/message name itself) names a
--- method on the receiver — typed here with `keyof self`, which gives
--- go-to-definition, hover, completion, and existence-checking on the handler string.
---@class AceEvent
local AceEvent = {}

--- Register for a game event. With a string `callback` the handler is `self[callback]`;
--- with a function it is called directly; when `callback` is omitted the handler is
--- the method named after the event (`self[event]`). Every handler is invoked as
--- `handler(event, ...)` — the event name followed by that event's payload — so an
--- inline function handler has its parameters typed from the event's payload.
---@generic E: FrameEvent
---@overload fun(self, event: FrameEvent & keyof self)
---@overload fun(self, event: FrameEvent, callback: keyof self, arg?: any)
---@param event E
---@param callback fun(event: FrameEvent, ...params<E>)
---@param arg? any @ prepended to the handler's arguments, before the event name
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0#title-3)
function AceEvent:RegisterEvent(event, callback, arg) end

--- Register for an inter-addon message. Handler resolution matches `RegisterEvent`.
---@overload fun(self, message: keyof self)
---@param message string
---@param callback keyof self | function
---@param arg? any @ prepended to the handler's arguments, before the message name
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0#title-5)
function AceEvent:RegisterMessage(message, callback, arg) end

---@param event FrameEvent
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0#title-4)
function AceEvent:UnregisterEvent(event) end

---@param message string
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0#title-6)
function AceEvent:UnregisterMessage(message) end

function AceEvent:UnregisterAllEvents() end

function AceEvent:UnregisterAllMessages() end

--- Send an inter-addon message to all registered handlers.
---@param message string
---@param ... any
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-event-3-0#title-7)
function AceEvent:SendMessage(message, ...) end
