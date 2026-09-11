---@diagnostic disable: create-global
-- Cross-file access modifier test: defines classes with private/protected fields

---@class AccessWidget
---@field name string
---@field private _secret string
---@field protected _internal number

---@return string
function AccessWidget:GetName()
    return self.name
end

---@param val string
function AccessWidget:_SetSecret(val)
    self._secret = val
end

-- A global @class namespace table, consumed cross-file by access_user.lua: its
-- private/protected fields must warn when touched from another file (the
-- declaring-file-is-clean side is covered by the namespace_privacy test).
---@class NsLib
---@field private callbackMap table<any, fun(stuff: any)>
---@field protected count number
NsLib = NsLib or {}

-- a field whose visibility is declared INLINE on the assignment (the sugar),
-- not via @field — its cross-file privacy must be enforced just the same
---@private
NsLib.token = NsLib.token or ""
