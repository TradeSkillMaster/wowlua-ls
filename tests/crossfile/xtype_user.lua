-- Cross-file @class + @type test: uses classes defined in xtype_defs.lua

-- Basic: @type annotation resolves cross-file @class fields
---@type XTypeVehicle
local vehicle = {}
local m = vehicle.make
--    ^ hover: (local) m: string  def: local
local y = vehicle.year
--    ^ hover: (local) y: number  def: local
local a = vehicle.active
--    ^ hover: (local) a: boolean  def: local

-- Inheritance: @class XTypeCar : XTypeVehicle inherits parent fields
---@type XTypeCar
local car = {}
local cm = car.make
--     ^ hover: (local) cm: string  def: local
local cd = car.doors
--     ^ hover: (local) cd: number  def: local

-- Nested class field: car.engine is XTypeEngine from same defs file
local eng = car.engine
--    ^ hover: (local) eng: XTypeEngine {  def: local
local hp = car.engine.horsepower
--     ^ hover: (local) hp: number  def: local
local fuel = car.engine.fuel
--     ^ hover: (local) fuel: string  def: local

-- Global function return type flows cross-file
local newCar = CreateCar()
--    ^ hover: (local) newCar: XTypeCar {  def: local
local nm = newCar.make
--     ^ hover: (local) nm: string  def: local

-- Global function @param type checking cross-file
local result = GetCarMake(car)
--     ^ hover: (local) result: string  def: local
