-- Cross-file test: @class with @field declarations for a mixin-style global

---@class MixinItem
---@field name string
---@field value number

---@return string
function MixinItem:GetName()
    return self.name
end
