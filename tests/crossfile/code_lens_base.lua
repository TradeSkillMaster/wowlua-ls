---@class Vehicle
local Vehicle = {}

---@param speed number
function Vehicle:SetSpeed(speed)
    self.speed = speed
end

function Vehicle:GetInfo()
    return "vehicle"
end
