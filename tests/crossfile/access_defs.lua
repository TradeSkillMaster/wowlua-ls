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
