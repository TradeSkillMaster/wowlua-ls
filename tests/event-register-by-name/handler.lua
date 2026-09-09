-- Register-by-name event handler typing (issue #58): a library whose registrar
-- takes the addon object as an ARGUMENT (not `self`) and the handler by string
-- name, typed `keyof T` in the overload. The handler method's parameters are
-- projected from the event payload, so it needs no `@param` annotations and its
-- params aren't `unknown-param-type` — both default-off codes are enabled here.
---@diagnostic disable: unused-local

---@class RBKeystoneInfo
---@field level number
---@field mapID number

---@event RBLibEvent
---| "KeystoneUpdate" -> unitName: string, keystoneInfo: RBKeystoneInfo, allInfo: table<string, RBKeystoneInfo>
---| "KeystoneAdded" -> id: string, entries: table<string, RBKeystoneInfo>
---| "KeystoneRemoved" -> id: string, entries: table<string, RBKeystoneInfo>

---@class RBOpenRaidLib
local lib = {}

---@generic T
---@generic E: RBLibEvent
---@overload fun(addonObject: T, event: E, callbackMemberName: keyof T)
---@param addonObject T
---@param event E
---@param callbackMemberName fun(...params<E>)
function lib.RegisterCallback(addonObject, event, callbackMemberName) end

---@class RBAddonObject
local addonObject = {}

-- Payload projected onto the named handler's params (no `@param` needed): the
-- overload types `callbackMemberName` as `keyof T`, so the handler owner is the
-- argument bound to `T` (`addonObject`), and the bound event's payload maps onto
-- the method's parameters positionally.
function addonObject.OnKeystoneUpdate(unitName, keystoneInfo, allInfo)
--                   ^ refs: 35:22, 46:54
    local u = unitName
--        ^ hover: (local) u: string
    local k = keystoneInfo
--        ^ hover: (local) k: RBKeystoneInfo
    local a = allInfo
--        ^ hover: (local) a: table<string, RBKeystoneInfo>
end

-- Find-references / rename on the handler method reaches the string name too.
lib.RegisterCallback(addonObject, "KeystoneUpdate", "OnKeystoneUpdate")
--                                                     ^ def: local 35:10

-- issue #60: the SAME named method registered for two STRUCTURALLY-IDENTICAL
-- events (both carrying a `table<K,V>` param) must NOT be read as a payload
-- conflict. Each event materializes its own table arena index for the map param,
-- so the conflict check compares payloads structurally, not by index — otherwise
-- the handler's params would revert to untyped.
function addonObject.OnKeystoneEntries(id, entries)
    local i = id
--        ^ hover: (local) i: string
    local e = entries
--        ^ hover: (local) e: table<string, RBKeystoneInfo>
end

lib.RegisterCallback(addonObject, "KeystoneAdded", "OnKeystoneEntries")
lib.RegisterCallback(addonObject, "KeystoneRemoved", "OnKeystoneEntries")
