-- Cross-file test: @field fun() types should be fully materialized,
-- enabling string literal completions and call resolution.

---@alias FFCEventName
---|"OnReady"
---|"OnComplete"
---|"OnError"

---@class FFCCallbackLib
---@field RegisterCallback fun(target: table, eventName: FFCEventName, handler: string|fun(eventName: FFCEventName, ...: unknown))
---@field UnregisterCallback fun(target: table, eventName: FFCEventName)
---@field OptionalCallback fun(eventName: FFCEventName)|nil
---@field LateinitCallback fun(eventName: FFCEventName)!

---@type FFCCallbackLib
local FFCCallbackLib = {}
