---@diagnostic disable: create-global
-- Saved variables an initializer builds, filling fields it defaults in place
-- (`t.f = t.f or {}`) and rewrites (`t.f = { ... }`).
function LedgerInitSaved()
    if LEDGER_DATA == nil then
        LEDGER_DATA = { Version = 2, Characters = {} }
    end
    LEDGER_DATA.Vaults = LEDGER_DATA.Vaults or { { bank = {}, money = 0 } }
    if LEDGER_DATA.Vaults.bank then
        LEDGER_DATA.Vaults = { { bank = LEDGER_DATA.Vaults.bank } }
    end
    LEDGER_DATA.Vaults[1].money = LEDGER_DATA.Vaults[1].money or 0
--                                                      ^ hover: (field) money: number
    LEDGER_FLAGS = LEDGER_FLAGS or {}
    LEDGER_FLAGS.modules = LEDGER_FLAGS.modules or {}
end
