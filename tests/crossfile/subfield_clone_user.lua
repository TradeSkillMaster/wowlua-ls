-- Cross-file test: access sub-fields on cloned shared class tables
-- Requires: --with-stubs

---@type SubFieldHost
local host = nil

-- Direct field access — should still resolve as Frame
local _sd = host.SpeedDisplay
--    ^ hover: (local) _sd: Frame {

-- Sub-field access — should find Speed on the cloned table, not undefined-field
local _spd = host.SpeedDisplay.Speed
--    ^ hover: (local) _spd: Cooldown {
--                             ^ diag: none

-- Second sub-field chain
local _txt = host.TextDisplay.Text
--    ^ hover: (local) _txt: Frame {
--                            ^ diag: none

-- Direct field still works on the parent
local _td = host.TextDisplay
--    ^ hover: (local) _td: Frame {
