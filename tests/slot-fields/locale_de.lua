---@diagnostic disable: create-global
-- Another locale creates the global if it doesn't exist yet and fills it.
if not LEDGER_L then LEDGER_L = {} end
local L = LEDGER_L

L.TITLE = "Kassenbuch"
L.EMPTY = "Leer"
L.USAGE = {
    "/ledger zeigen",
    "/ledger verbergen",
}
