-- Cross-file inheritance test: defines parent classes used in inherit_user.lua

---@class InhShape
---@field color string
---@field visible boolean

---@return string
function InhShape:GetColor()
    return self.color
end

---@class InhRect : InhShape
---@field width number
---@field height number

---@return number
function InhRect:Area()
    return self.width * self.height
end
