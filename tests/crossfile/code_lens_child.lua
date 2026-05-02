---@class Car : Vehicle
-- ^ lens: 0 implementations
local Car = {}

function Car:SetSpeed(speed)
--          ^ lens: SetSpeed, overrides Vehicle
    self.speed = speed * 2
end

function Car:Honk()
--          ^ lens: Honk
    return "beep"
end
