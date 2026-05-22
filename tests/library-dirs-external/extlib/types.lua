-- External library: provides types but lives outside the workspace.

---@class ExtWidget
---@field id number
---@field label string

---@param w ExtWidget
---@return string
function FormatWidget(w)
    local unused = true
    return w.label
end
