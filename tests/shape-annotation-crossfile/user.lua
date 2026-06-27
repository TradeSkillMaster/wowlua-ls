---@diagnostic disable: unused-local
-- The standalone `@shape Item ...` in shapes.lua attaches to Item, so these plain
-- tables are accepted where an Item is expected — even though Item keeps its
-- IsValid method and all data fields for member access.

-- Accepted: matches shape member 1 (bag + slot).
UseItem({ bagID = 0, slotIndex = 1 })

-- Accepted: matches shape member 2 (equipment slot).
UseItem({ equipmentSlotIndex = 5 })

-- Rejected: an unrelated table matches no shape (type-mismatch, since a
-- shape-bearing class is matched solely by its shapes).
UseItem({ foo = 1 })
--      ^ diag: type-mismatch

-- Item itself still has its fields/method (the shape augmented, not replaced it).
-- The standalone shape also drives read-side nilability through the build path
-- (EXT_BASE index mapping): each location field is absent from one shape member,
-- so it reads as conditionally present (number?).
---@type Item
local real = nil
local _ok = real.IsValid
local _bag = real.bagID
--    ^ hover: (local) _bag: number?
local _eq = real.equipmentSlotIndex
--    ^ hover: (local) _eq: number?
