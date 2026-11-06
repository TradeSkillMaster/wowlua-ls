---@diagnostic disable: create-global
-- A saved variable declared by a `---@type` after its `nil` initializer and
-- defaulted in a load handler.
---@class LedgerTestSettings
---@field Anonymize boolean

LEDGER_TEST_SETTINGS = nil ---@type LedgerTestSettings

function LedgerVarsLoaded()
    LEDGER_TEST_SETTINGS = LEDGER_TEST_SETTINGS or {}
end
