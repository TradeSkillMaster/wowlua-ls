-- Cross-file test: @class with @field table<K,V>

---@class XWidget
---@field visible boolean

---@return string
function XWidget:GetName()
    return "widget"
end

---@class XWidgetPool
---@field index number
---@field pool table<number, XWidget>
