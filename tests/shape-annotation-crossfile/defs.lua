---@diagnostic disable: create-global, unused-local
-- A userdata-mixin-style class with data fields plus a method. Defined in one
-- file; the `@shape` is declared standalone in another (shapes.lua), mirroring
-- how a stub override attaches a shape to a generated class additively.

---@class Item
---@field bagID number
---@field slotIndex number
---@field equipmentSlotIndex number
---@field IsValid fun(self: Item): boolean

---@param it Item
function UseItem(it) return it.bagID end
