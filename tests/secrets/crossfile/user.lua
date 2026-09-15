---@diagnostic disable: unused-local, empty-block
-- Secret-value metadata and taint declared in another file (`defs.lua`).
local _, ns = ...
local hp = UnitHealth("target")

-- Guards declared in another file narrow.
if Shared_CanAccess(hp) then
    local _ = hp > 0
end
if not ns.Helper:IsSecret(hp) then
    local _ = hp > 1
end

-- Secrecy of another file's function returns and fields.
local inferred = Shared_TargetHealth()
--    ^ hover: (local) inferred: secret<number>
if inferred > 2 then end
-- ^ diag: secret-comparison ~value from `Shared_TargetHealth` may be secret
local stored = ns.lastHealth
--    ^ hover: (local) stored: secret<number>

-- `@secret-unless` and `@secret-args` from another file.
local own = Shared_NameOf("player")
--    ^ hover: (local) own: string
local other = Shared_NameOf("target")
--    ^ hover: (local) other: secret<string>
Shared_Send(other)
--          ^ diag: secret-argument
