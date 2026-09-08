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
--                   ^ refs: 33:22, 44:54
    local u = unitName
--        ^ hover: (local) u: string
    local k = keystoneInfo
--        ^ hover: (local) k: RBKeystoneInfo
    local a = allInfo
--        ^ hover: (local) a: table
end

-- Find-references / rename on the handler method reaches the string name too.
lib.RegisterCallback(addonObject, "KeystoneUpdate", "OnKeystoneUpdate")
--                                                     ^ def: local 33:10
