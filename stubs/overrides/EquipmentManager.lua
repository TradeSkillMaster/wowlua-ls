---@meta _

-- EquipmentManager_UnpackLocation was deprecated on retail in 11.2.0 (Ketho's
-- Deprecated_11_2_0.lua stubs it with a @param but NO @return), yet it still
-- returns its values at runtime and remains the live API on Classic. The retail
-- form returns six values (isOnPlayer, isInBank, isInBags, isInVoidStorage,
-- slot, bag); Classic's Wrath FrameXML body returns five (no isInVoidStorage).
-- The six-value form is not present in any cloned source — retail dropped the
-- Lua body and the Classic body has only five — so the FrameXML inferred-returns
-- pass (which also scans retail only) cannot recover it. Declaring it here is
-- the only way to give it the full arity; without any @return, destructuring its
-- result false-positives as `unbalanced-assignments`.
--
-- Positions 4-6 are typed `number` so the slot/bag values land as numbers under
-- BOTH destructure idioms addons use per flavor: `p, b, bags, slot, bag` on
-- Classic (slot/bag at positions 4/5) and `p, b, bags, _, slot, bag` on retail
-- (slot/bag at positions 5/6, with the discarded isInVoidStorage at position 4).
-- @deprecated is preserved from Ketho's stub; it is flavor-suppressed for addons
-- that also target Classic (where the function is current).

---@deprecated
---Deprecated by [EquipmentManager_GetLocationData](https://www.townlong-yak.com/framexml/go/EquipmentManager_GetLocationData)
---@param packedLocation number
---@return boolean isOnPlayer
---@return boolean isInBank
---@return boolean isInBags
---@return number isInVoidStorageOrSlot
---@return number slotOrBag
---@return number bag
function EquipmentManager_UnpackLocation(packedLocation) end
