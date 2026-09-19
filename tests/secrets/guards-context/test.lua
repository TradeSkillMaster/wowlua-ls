---@diagnostic disable: unused-local, unused-function, empty-block, redundant-condition
-- Context guards: `C_Secrets` predicates (`@secret-clears`) and addon
-- restrictions (`@secret-restriction-guard`) clear secrecy in the code they
-- guard, for the predicates they cover.

local unit = "target"

-- ── Predicate guards ───────────────────────────────────────────────────────────

if not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
    local max = UnitHealthMax(unit)
    --    ^ hover: (local) max: number
    --       ^ hint: : number
    if max > 0 then end
    local maxText = UnitHealthMax(unit)
    --              ^ doc: !**Secrecy**
    -- A different unit than the guarded one.
    local focusMax = UnitHealthMax("focus")
    --    ^ hover: (local) focusMax: secret<number>
    if focusMax > 0 then end
    -- ^ diag: secret-comparison
    -- A predicate the guard doesn't cover.
    local hp = UnitHealth(unit)
    if hp > 0 then end
    -- ^ diag: secret-comparison
    -- Code that runs later stays secret.
    local frame = CreateFrame("Frame")
    frame:SetScript("OnUpdate", function()
        if UnitHealthMax(unit) > 0 then end
        -- ^ diag: secret-comparison
    end)
    if UnitHealthMax(unit) > 0 then end
    --              ^ sig: fun(unit: UnitTokenPvPRestrictedForAddOns): number
end

-- A value produced before the guard stays secret.
local earlyMax = UnitHealthMax(unit)
if not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
    if earlyMax > 0 then end
    -- ^ diag: secret-comparison
end

-- The true branch reports; the else branch is clear.
if C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
    if UnitHealthMax(unit) > 0 then end
    -- ^ diag: secret-comparison
else
    if UnitHealthMax(unit) > 0 then end
end

-- Code after the guarded block reports.
if UnitHealthMax(unit) > 0 then end
-- ^ diag: secret-comparison

-- `and` / `or` right operands, and `elseif` conditions and bodies.
local andGuarded = not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) and UnitHealthMax(unit) > 0
local orGuarded = C_Secrets.ShouldUnitHealthMaxBeSecret(unit) or UnitHealthMax(unit) > 0
if C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
elseif UnitHealthMax(unit) > 0 then
    local elseifMax = UnitHealthMax(unit) + 1
end
if unit == "focus" then
elseif not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) and UnitHealthMax(unit) > 0 then end

-- `assert`, early exits, and loops.
local function Asserted(u)
    assert(not C_Secrets.ShouldUnitHealthMaxBeSecret(u))
    return UnitHealthMax(u) + 1
end

local function EarlyExit(u)
    local before = UnitHealthMax(u)
    if before > 0 then end
    -- ^ diag: secret-comparison
    if C_Secrets.ShouldUnitHealthMaxBeSecret(u) then return end
    local after = UnitHealthMax(u)
    --    ^ hover: (local) after: number
    if after > 0 then end
    u = "focus"
    if UnitHealthMax(u) > 0 then end
    -- ^ diag: secret-comparison
end

while not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) do
    if UnitHealthMax(unit) > 0 then break end
end

-- A field chain binds like a variable, until any link of it is written.
local Bar = { unit = "boss1", data = { unit = "boss1" } }
function Bar:Update()
    if C_Secrets.ShouldUnitHealthMaxBeSecret(self.unit) then return end
    if UnitHealthMax(self.unit) > 0 then end
    self.unit = "boss2"
    if UnitHealthMax(self.unit) > 0 then end
    -- ^ diag: secret-comparison
end
function Bar:Outer()
    if C_Secrets.ShouldUnitHealthMaxBeSecret(self.data.unit) then return end
    self.data = { unit = "focus" }
    if UnitHealthMax(self.data.unit) > 0 then end
    -- ^ diag: secret-comparison
end
function Bar:Looped()
    if C_Secrets.ShouldUnitHealthMaxBeSecret(self.unit) then return end
    for _ = 1, 3 do
        if UnitHealthMax(self.unit) > 0 then end
        -- ^ diag: secret-comparison
        self.unit = "boss2"
    end
end

-- The existence check older clients need doesn't hide the guard.
if C_Secrets and C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
else
    if UnitHealthMax(unit) > 0 then end
end

-- A branch or loop merge may hold a reassignment, so it binds nothing.
local merged = "target"
if not C_Secrets.ShouldUnitHealthMaxBeSecret(merged) then
    if unit == "focus" then merged = "focus" else merged = "boss1" end
    if UnitHealthMax(merged) > 0 then end
    -- ^ diag: secret-comparison
end

-- An argument's secrecy still flows through a cleared `@secret-args tainted` callee.
if not C_Secrets.ShouldAurasBeSecret() then
    local plainDuration = C_UnitAuras.GetAuraBaseDuration(unit, 1)
    if plainDuration and plainDuration > 0 then end
    local taintedDuration = C_UnitAuras.GetAuraBaseDuration(unit, UnitHealth(unit))
    if taintedDuration and taintedDuration > 0 then end
    --                     ^ diag: secret-comparison
end

-- A namespace cached in a local resolves too.
local Secrets = C_Secrets
if not Secrets.ShouldUnitHealthMaxBeSecret(unit) then
    if UnitHealthMax(unit) > 0 then end
end

-- Pair-bound predicates need both units to match.
local mob = "nameplate1"
if not C_Secrets.ShouldUnitThreatStateBeSecret(unit, mob) then
    local situation = UnitThreatSituation(unit, mob)
    if situation == 3 then end
    local otherSituation = UnitThreatSituation(unit, "nameplate2")
    if otherSituation == 3 then end
    -- ^ diag: secret-comparison
end

-- Struct fields clear with their class's predicates (not bound to the unit).
if not C_Secrets.ShouldAurasBeSecret() then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --               ^ hover: (field) duration: number
    if UnitHealthMax(unit) > 0 then end
    -- ^ diag: secret-comparison
end

-- A base-secrecy comparison clears the spell's cooldown.
local spellID = 61304
if C_Secrets.GetSpellCooldownSecrecy(spellID) == Enum.SecrecyLevel.NeverSecret then
    local cooldown = C_Spell.GetSpellCooldown(spellID)
    if cooldown and cooldown.duration > 0 then end
end

-- `HasSecretRestrictions` false: nothing is secret, even values from before.
local earlierHp = UnitHealth("target")
if not C_Secrets.HasSecretRestrictions() then
    local hp = UnitHealth("target") + 1
    --    ^ hover: (local) hp: number
    if earlierHp > 0 then end
    local health = UnitHealth("focus")
    --             ^ doc: !**Secrecy**
end

-- ── Restriction guards ─────────────────────────────────────────────────────────

-- Combat alone clears `SecretWhenInCombat` but not the aura predicates, which
-- also depend on encounters, challenge modes, and PvP matches.
if not C_RestrictedActions.IsAddOnRestrictionActive(Enum.AddOnRestrictionType.Combat) then
    local session = C_DamageMeter.GetCombatSessionFromType(Enum.DamageMeterSessionType.Overall, Enum.DamageMeterType.DamageDone)
    if session.totalAmount and session.totalAmount > 0 then end
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --          ^ diag: secret-comparison
end

-- `InCombatLockdown` is the combat restriction.
if not InCombatLockdown() then
    local session = C_DamageMeter.GetCombatSessionFromType(Enum.DamageMeterSessionType.Overall, Enum.DamageMeterType.DamageDone)
    if session.totalAmount and session.totalAmount > 0 then end
end

-- Three of the four aura restrictions still leave auras secret; all four clear them,
-- written with a local alias of the enum and `GetAddOnRestrictionState`.
local RType = Enum.AddOnRestrictionType
if not InCombatLockdown() and not C_RestrictedActions.IsAddOnRestrictionActive(RType.Encounter)
    and C_RestrictedActions.GetAddOnRestrictionState(RType.ChallengeMode) == Enum.AddOnRestrictionState.Inactive then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --          ^ diag: secret-comparison
    if not C_RestrictedActions.IsAddOnRestrictionActive(RType.PvPMatch) then
        if aura and aura.duration > 5 then end
        --               ^ hover: (field) duration: number
        local cooldown = C_Spell.GetSpellCooldown(spellID)
        if cooldown and cooldown.duration > 0 then end
    end
end

-- `Activating` counts as active.
if C_RestrictedActions.GetAddOnRestrictionState(Enum.AddOnRestrictionType.Combat) ~= Enum.AddOnRestrictionState.Active then
    local session = C_DamageMeter.GetCombatSessionFromType(Enum.DamageMeterSessionType.Overall, Enum.DamageMeterType.DamageDone)
    if session.totalAmount and session.totalAmount > 0 then end
    --                         ^ diag: secret-comparison
end

-- Unit-condition predicates are cleared only by their own guard.
if not InCombatLockdown() then
    if UnitHealthMax(unit) > 0 then end
    -- ^ diag: secret-comparison
end

-- ── Guards held in a variable ─────────────────────────────────────────────────

local aurasSecret = C_Secrets.ShouldAurasBeSecret()
if not aurasSecret then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --               ^ hover: (field) duration: number
end
if aurasSecret then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --          ^ diag: secret-comparison
end
-- A later write with no guard drops what the variable carried.
local staleGuard = C_Secrets.ShouldAurasBeSecret()
staleGuard = UnitAffectingCombat(unit)
if not staleGuard then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --          ^ diag: secret-comparison
end

-- A write inside a branch proves nothing after the chain: the compat shim's other
-- branch leaves the variable holding something else.
local shimSecret
if C_Secrets then
    shimSecret = C_Secrets.ShouldAurasBeSecret()
else
    shimSecret = UnitAffectingCombat(unit)
end
if not shimSecret then
    local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
    if aura and aura.duration > 5 then end
    --          ^ diag: secret-comparison
end

-- ── Arguments bound by the callee's parameter name ────────────────────────────

-- The power APIs take the power type second, so the guard reaches them by name.
local power = Enum.PowerType.Mana
if C_Secrets.GetPowerTypeSecrecy(power) == Enum.SecrecyLevel.NeverSecret then
    local mana = UnitPower(unit, power)
    --    ^ hover: (local) mana: number
    local rage = UnitPower(unit, Enum.PowerType.Rage)
    --    ^ hover: (local) rage: secret<number>
end

-- The threat APIs name their second unit `mobGUID`.
if not C_Secrets.ShouldUnitThreatStateBeSecret(unit, "boss1") then
    local threat = UnitThreatSituation(unit, "boss1")
    --    ^ hover: (local) threat: number?
    local otherThreat = UnitThreatSituation(unit, "boss2")
    --    ^ hover: (local) otherThreat: secret<number>?
end

-- ── Preconditions a guard proves ──────────────────────────────────────────────

local unguardedSame = UnitIsUnit("player", unit)
--    ^ hover: (local) unguardedSame: secret<boolean>?
if C_Secrets.CanCompareUnitTokens("player", unit) then
    local same = UnitIsUnit("player", unit)
    --    ^ hover: (local) same: secret<boolean>
    local other = UnitIsUnit("party1", unit)
    --    ^ hover: (local) other: secret<boolean>?
end
