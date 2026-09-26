---@diagnostic disable: unused-local, empty-block
-- Secret values from the generated WoW API stubs (retail). No config: an addon
-- with no flavor signal counts as retail.

-- ── Stub-derived taint ─────────────────────────────────────────────────────────

local hp = UnitHealth("target")
--    ^ hover: (local) hp: secret<number>

local name, realm = UnitName("target")
--          ^ hover: (local) realm: secret<string>

-- NeverSecret returns stay plain.
local _, _, _, _, _, isTradeskill, _, _, _, castBarID = UnitCastingInfo("target")
--                   ^ hover: (local) isTradeskill: boolean

-- Structure fields of a secret-returning API.
local cooldown = C_Spell.GetSpellCooldown(61304)
local duration = cooldown and cooldown.duration
--    ^ hover: (local) duration: secret<number>

-- `@secret-unless`: the player's own identity is never secret, also through
-- `select`.
local playerName = UnitName("player")
--    ^ hover: (local) playerName: string
local _, playerClass = UnitClass("player")
--       ^ hover: (local) playerClass: string
local classID = select(3, UnitClass("player"))
--    ^ hover: (local) classID: number
-- Cast and max-power secrecy doesn't apply to the player's units either.
local _, _, _, _, castEndTimeMs = UnitCastingInfo("player")
--                ^ hover: (local) castEndTimeMs: number
local castRemaining = (castEndTimeMs - GetTime()) / 1000
local _, _, _, _, channelEndTimeMs = UnitChannelInfo("pet")
--                ^ hover: (local) channelEndTimeMs: number
local playerMaxPower = UnitPowerMax("player")
--    ^ hover: (local) playerMaxPower: number
local _, _, _, _, focusCastEndTimeMs = UnitCastingInfo("focus")
--                ^ hover: (local) focusCastEndTimeMs: secret<number>

-- Fields of structures Blizzard's docs don't describe come from the wiki: every
-- `AuraData` field may be secret except the ones marked NeverSecret.
local aura = C_UnitAuras.GetAuraDataByIndex("target", 1, "HARMFUL")
if aura then
    local auraDuration = aura.duration
    --    ^ hover: (local) auraDuration: secret<number>
    local auraInstanceID = aura.auraInstanceID
    --    ^ hover: (local) auraInstanceID: number
end

-- Widget getters aren't tainted (only frames fed secrets return them).
local frame = CreateFrame("Frame")
local width = frame:GetWidth()
--    ^ hover: (local) width: number

-- Event payloads.
frame:SetScript("OnEvent", function(self, event, ...)
    if event == "UNIT_SPELLCAST_SENT" then
        local unit, target = ...
        --          ^ hover: (local) target: secret<string>
        local _ = target .. ""
        local realmStart = target:find("-")
        --                 ^ diag: secret-access ~calling a method on it
    elseif event == "UNIT_AURA" then
        local unit, updateInfo = ...
        if updateInfo and updateInfo.addedAuras then
            for _, added in ipairs(updateInfo.addedAuras) do
                local addedName = added.name
                --    ^ hover: (local) addedName: secret<string>
            end
        end
    end
end)

-- `@secret-args tainted` APIs return results carrying their arguments' secrecy.
local text = string.format("%d", hp)
--    ^ hover: (local) text: secret<string>
--                  ^ doc: Secret arguments: `AllowedWhenTainted` — accepted (results inherit their secrecy)
local plain = string.format("%d", 1)
--    ^ hover: (local) plain: string

-- ── Hover: Secrecy section ─────────────────────────────────────────────────────

local c1, c2, c3 = UnitClass("focus")
--                 ^ doc: May return secret values: `SecretWhenUnitIdentityRestricted` — Guarded APIs and events produce secret values when the unit isn't player-controlled
local c4 = UnitClass("focus")
--         ^ doc: Never secret when `unit` is `"player"` or `"pet"`
local h1 = UnitHealth("focus")
--         ^ doc: Returns may be secret values.
local ci = UnitCastingInfo("focus")
--         ^ doc: Never secret: `isTradeskill`, `castBarID`, `delayTimeMs`
--         ^ doc: Never secret when `unit` is `"player"` or `"pet"`
local pm = UnitPowerMax("focus")
--         ^ doc: Never secret when `unitToken` is `"player"` or `"pet"`
local s1 = issecretvalue(hp)
--         ^ doc: Guard: `true` means `value` is secret
frame:SetAlpha(0.5)
--    ^ doc: Secret aspect `Alpha`: passing a secret value marks the widget's aspect secret
--    ^ doc: Secret arguments: `AllowedWhenTainted` — accepted
C_ChatInfo.SendAddonMessage("PFX", "hello", "PARTY")
--         ^ doc: Secret arguments: `NotAllowed` — never accepted
local plainDoc = GetTime()
--               ^ doc: !**Secrecy**
frame:RegisterEvent("UNIT_SPELLCAST_SENT")
--                   ^ doc: Payload may be secret: `SecretWhenUnitSpellCastRestricted`

-- Constant accessors return secrets exactly when an argument is secret.
local formatter = C_StringUtil.CreateSecondsFormatter()
local remainingText = formatter:Format(UnitHealth("focus"))
--    ^ hover: (local) remainingText: secret<string>
local fixedText = formatter:Format(5)
--    ^ hover: (local) fixedText: string
local curve = C_CurveUtil.CreateCurve()
local curved = curve:Evaluate(UnitHealth("focus"))
--    ^ hover: (local) curved: secret<number>

-- Preconditions: a failing `ReturnNothing`/`ReturnWithError` one makes the returns nilable.
local sameUnit = UnitIsUnit("target", "focus")
--    ^ hover: (local) sameUnit: secret<boolean>?
--               ^ doc: Returns nothing when `RequiresComparableUnitTokens` fails — Guarded APIs only accept unit token pairs
local effectiveAlpha = frame:GetEffectiveAlpha()
--    ^ hover: (local) effectiveAlpha: number?
--                           ^ doc: Returns nothing and reports an error when `RequiresScriptObjectAlphaAccess` fails
local auraByIndex = C_UnitAuras.GetAuraDataByIndex("target", 1)
--                              ^ doc: Errors when `RequiresUnitAuraAccess` fails

-- Context guards.
local healthMaxSecret = C_Secrets.ShouldUnitHealthMaxBeSecret("target")
--                                ^ doc: Guard: `false` clears `SecretWhenUnitHealthMaxRestricted` for later calls with the same `unit`
local secretsEnabled = C_Secrets.HasSecretRestrictions()
--                               ^ doc: Guard: `false` means no API returns secret values
local cooldownSecrecy = C_Secrets.GetSpellCooldownSecrecy(61304)
--                                ^ doc: Guard: `Enum.SecrecyLevel.NeverSecret` clears `SecretWhenCooldownsRestricted` for later calls with the same `spellIdentifier`
local encounterActive = C_RestrictedActions.IsAddOnRestrictionActive(Enum.AddOnRestrictionType.Encounter)
--                                          ^ doc: Guard: `false` means the restriction passed as `type` is inactive
local lockdown = InCombatLockdown()
--               ^ doc: Guard: `false` means the `Combat` restriction is inactive

-- ── Diagnostics on stub-derived values ─────────────────────────────────────────

local maxHp = UnitHealthMax("target")
local pct = hp / maxHp
--          ^ diag: secret-arithmetic ~value from `UnitHealth` may be secret
if hp < maxHp then end
-- ^ diag: secret-comparison
local roleColors = {}
local role = UnitGroupRolesAssigned("target")
local color = roleColors[role]
--                       ^ diag: secret-table-key ~value from `UnitGroupRolesAssigned` may be secret
if UnitIsAFK("target") then end
-- ^ diag: secret-condition ~boolean from `UnitIsAFK` may be secret

local focusName = UnitName("focus")
local shortFocusName = focusName:sub(1, 3)
--                     ^ diag: secret-access ~value from `UnitName` may be secret; calling a method on it errors in addon code
local focusNameLength = #UnitName("focus")
--                      ^ diag: secret-access ~value from `UnitName` may be secret; taking its length
for i = 1, UnitHealth("focus") do end
--         ^ diag: secret-comparison ~value from `UnitHealth` may be secret; using it as a `for` loop bound
local playerNameUpper = UnitName("player"):upper()
for i = 1, UnitHealthMax("player") do end
local focusCastRemaining = focusCastEndTimeMs - GetTime()
--                         ^ diag: secret-arithmetic ~value from `UnitCastingInfo` may be secret
local aurasById = {}
if aura then
    local byInstance = aurasById[aura.auraInstanceID]
    if aura.isHarmful then end
    local bySpell = aurasById[aura.spellId]
    --                        ^ diag: secret-table-key ~value from `AuraData.spellId` may be secret
    local auraRemaining = aura.expirationTime - GetTime()
    --                    ^ diag: secret-arithmetic ~value from `AuraData.expirationTime` may be secret
end
-- Narrowing `aura` leaves its fields secret.
if aura and aura.duration > 5 then end
--          ^ diag: secret-comparison ~value from `AuraData.duration` may be secret

-- A guard's true branch still reports what errors there.
local function SecretBranch()
    local raw = UnitHealth("target")
    if issecretvalue(raw) then
        if raw > 0 then end
        -- ^ diag: secret-comparison
    end
end

-- A guard in a `repeat` body also covers its `until` condition.
local function SecretRepeat()
    repeat
        local raw = UnitHealth("target")
        if not canaccessvalue(raw) then return end
    until raw > 0
    repeat
        local raw = UnitHealth("target")
    until raw > 0
    --    ^ diag: secret-comparison
end

-- Guarded by the builtins: no diagnostics.
if issecretvalue(hp) or issecretvalue(maxHp) then return end
if hp < maxHp then end
local colorsByClass = {}
local _, targetClass = UnitClass("target")
if canaccessvalue(targetClass) then
    local _ = colorsByClass[targetClass]
end
if canaccessallvalues(hp, maxHp) then
    local _ = hp / maxHp
end
local polyfill = issecretvalue or function() return false end
local targetName = UnitName("target")
if not polyfill(targetName) then
    local _ = roleColors[targetName]
end
if canaccessvalue(targetName) then
    local _ = targetName:upper()
    local _ = #targetName
end

-- The player's identity is never secret.
if playerClass == "WARRIOR" then end
local _ = colorsByClass[playerClass]

-- ── Untainted APIs: addon code may not pass secrets ────────────────────────────

local targetGuid = UnitGUID("target")
local targetExists = UnitExists(targetGuid)
--                   ^ doc: !**Secrecy**
--                              ^ diag: secret-argument ~`UnitExists` does not accept secret values from addon code
local playerExists = UnitExists("player")
if canaccessvalue(targetGuid) then
    local guardedExists = UnitExists(targetGuid)
end

-- `math.*`: secure code may do arithmetic on a secret, addon code may not. (`hp`
-- is narrowed to a plain number by the guard above, so fetch a fresh one.)
local focusHp = UnitHealth("focus")
local floored = math.floor(focusHp)
--                         ^ diag: secret-argument ~`math.floor` does not accept secret values from addon code
local plainFloored = math.floor(1.5)

-- The builtins that exist to work with secrets take them from anywhere.
local isHpSecret = issecretvalue(focusHp)
local bothReadable = canaccessallvalues(focusHp, maxHp)
local anySecret = hasanysecretvalues(focusHp)
local scrubbedHp = scrub(focusHp)
local dumpedHp = dumpobject(focusHp)

-- `tostring` returns a secret string; `tonumber` an ordinary number.
local hpText = tostring(focusHp)
--    ^ hover: (local) hpText: secret<string>
if hpText == "0" then end
--  ^ diag: secret-comparison ~value from `tostring` may be secret
local hpNumber = tonumber(hpText)
--    ^ hover: (local) hpNumber: number?

-- `secretwrap` fabricates a secret; `secretunwrap` gives an ordinary value back.
local fakeHealth = secretwrap(5)
--    ^ hover: (local) fakeHealth: secret<number>
local realHealth = secretunwrap(fakeHealth)
--    ^ hover: (local) realHealth: number
local plainName = secretunwrap(UnitName("target"))
--    ^ hover: (local) plainName: string

-- ── Lua library: number parameters reject secrets, string ones are untouched ───

local libHp = UnitHealth("focus")
local libName = UnitName("focus")
local repeated = string.rep("ab", libHp)
--                                ^ diag: secret-argument ~`string.rep` does not accept secret values from addon code
local sliced = string.sub("abc", libHp)
--                               ^ diag: secret-argument ~`string.sub` does not accept secret values from addon code
local picked = select(libHp, "a", "b")
--                    ^ diag: secret-argument ~`select` does not accept secret values from addon code
local removed = table.remove({}, libHp)
--                               ^ diag: secret-argument ~`table.remove` does not accept secret values from addon code
-- A string parameter of the same function takes a different conversion.
local repeatedName = string.rep(libName, 2)
local joinedName = string.format("%s", libName)
--    ^ hover: (local) joinedName: secret<string>
-- Plain numbers, and a guarded secret, pass.
local plainRepeat = string.rep("ab", 2)
if canaccessvalue(libHp) then
    local guardedRepeat = string.rep("ab", libHp)
end
