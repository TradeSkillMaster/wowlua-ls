-- Cross-file @class + @type test: defines classes consumed in xtype_user.lua

---@class XTypeVehicle
---@field make string
---@field year number
---@field active boolean

---@class XTypeEngine
---@field horsepower number
---@field fuel string

---@class XTypeCar : XTypeVehicle
---@field engine XTypeEngine
---@field doors number

---@return XTypeCar
function CreateCar()
    return {}
end

---@param car XTypeCar
---@return string
function GetCarMake(car)
    return car.make
end
