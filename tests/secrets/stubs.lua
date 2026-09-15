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

-- A guard's true branch still reports what errors there.
local function SecretBranch()
    local raw = UnitHealth("target")
    if issecretvalue(raw) then
        if raw > 0 then end
        -- ^ diag: secret-comparison
    end
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

-- The player's identity is never secret.
if playerClass == "WARRIOR" then end
local _ = colorsByClass[playerClass]
