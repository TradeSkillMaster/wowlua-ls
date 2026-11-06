---@diagnostic disable: unused-local, unused-function, create-global
---@return number
function LedgerMoney() return 0 end

local function UpdateMoney()
    LEDGER_DATA.Vaults[1].money = LedgerMoney()
--                        ^ hover: (field) money: number
    local vault = LEDGER_DATA.Vaults[1]
    local money = vault.money
--        ^ hover: (local) money: number
end

local function ModuleEnabled(name)
    return LEDGER_FLAGS.modules[name]
end

local function SetModule(name, enabled)
    LEDGER_FLAGS.modules[name] = enabled or nil
end
