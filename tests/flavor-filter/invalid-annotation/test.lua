-- Validation of the `@flavor-narrows` annotation itself. No `.wowluarc.json`
-- in this directory — flavor filtering is off, so we only exercise the
-- malformed-annotation path.

-- Bad flavor name → malformed-annotation
---@flavor-narrows wrath
-- ^ diag: malformed-annotation
---@return boolean
local function Bad1() return true end

-- Empty → malformed-annotation
---@flavor-narrows
-- ^ diag: malformed-annotation
---@return boolean
local function Bad2() return true end

-- Mixed: one valid + one invalid → still malformed (unknown names are listed).
---@flavor-narrows retail, cataclysm
-- ^ diag: malformed-annotation
---@return boolean
local function Bad3() return true end

-- All valid → no diagnostic on the annotation.
---@flavor-narrows retail, classic
-- ^ diag: none
---@return boolean
local function Good1() return true end

-- Canonical single flavor → no diagnostic.
---@flavor-narrows classic_era
-- ^ diag: none
---@return boolean
local function Good2() return true end

return { Bad1, Bad2, Bad3, Good1, Good2 }
