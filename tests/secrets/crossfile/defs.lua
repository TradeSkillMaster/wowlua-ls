---@diagnostic disable: create-global, unused-local
-- Secret-value declarations consumed from `user.lua`.
local _, ns = ...

---@secret-guard value accessible
---@param value any
---@return boolean
function Shared_CanAccess(value) return canaccessvalue(value) end

---@class SecretHelper
local Helper = {}
ns.Helper = Helper

---@secret-guard value is-secret
---@param value any
---@return boolean
function Helper:IsSecret(value) return issecretvalue(value) end

function Shared_TargetHealth()
    return UnitHealth("target")
end

---@secret-unless unit player
---@param unit string
---@return string|secret<string>
function Shared_NameOf(unit) return "" end

---@secret-args none
---@param text string
function Shared_Send(text) end

ns.lastHealth = UnitHealth("target")

---@secret-clears SecretWhenGaugeRestricted gauge
---@param gauge string
---@return boolean
function Shared_ShouldGaugeBeSecret(gauge) return true end

---@secret-when SecretWhenGaugeRestricted
---@param gauge string
---@return secret<number>
function Shared_GaugeValue(gauge) return 1 end

---@class SharedGaugeInfo
---@secret-when SecretWhenGaugeRestricted
---@field current secret<number>

---@return SharedGaugeInfo
function Shared_GaugeInfo() return { current = 1 } end
