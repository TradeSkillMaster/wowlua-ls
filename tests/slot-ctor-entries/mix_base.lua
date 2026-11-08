---@diagnostic disable: create-global
-- A table two flavor files each build under one path (`mix_legacy.lua`,
-- `mix_modern.lua`), an entry a literal in one and a call in the other.
SlotMix = { Cfg = {} }

---@return number
function SlotMixMode() return 1 end
