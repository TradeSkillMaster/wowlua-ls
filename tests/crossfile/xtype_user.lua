-- Cross-file @class + @type test: uses classes defined in xtype_defs.lua

-- Basic: @type annotation resolves cross-file @class fields
---@type XTypeVehicle
local vehicle = {}
local m = vehicle.make
--    ^ hover: (global) m: string  def: local
local y = vehicle.year
--    ^ hover: (global) y: number  def: local
local a = vehicle.active
--    ^ hover: (global) a: boolean  def: local

-- Inheritance: @class XTypeCar : XTypeVehicle inherits parent fields
---@type XTypeCar
local car = {}
local cm = car.make
--     ^ hover: (global) cm: string  def: local
local cd = car.doors
--     ^ hover: (global) cd: number  def: local

-- Nested class field: car.engine is XTypeEngine from same defs file
local eng = car.engine
--    ^ hover: (global) eng: XTypeEngine {  def: local
local hp = car.engine.horsepower
--     ^ hover: (global) hp: number  def: local
local fuel = car.engine.fuel
--     ^ hover: (global) fuel: string  def: local

-- Global function return type flows cross-file
local newCar = CreateCar()
--    ^ hover: (global) newCar: XTypeCar {  def: local
local nm = newCar.make
--     ^ hover: (global) nm: string  def: local

-- Global function @param type checking cross-file
local result = GetCarMake(car)
--     ^ hover: (global) result: string  def: local
