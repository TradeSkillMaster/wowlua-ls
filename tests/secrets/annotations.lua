---@diagnostic disable: unused-local, unused-function, empty-block
-- Secret-value rules through user annotations: `secret<T>` types, propagation,
-- `@secret-guard` narrowing, and the `secret-*` diagnostics.

---@return number|secret<number>
local function GetHealth() return 1 end

---@return boolean|secret<boolean>
local function IsFlagged() return true end

---@return string|secret<string>
local function GetLabel() return "" end

---@type secret<number>
local definite = 1

---@secret-guard value is-secret
---@param value any
---@return boolean
local function IsSecret(value) return false end

---@secret-guard value accessible
---@param value any
---@return boolean
local function CanAccess(value) return true end

---@secret-guard ... accessible
---@param ... any
---@return boolean
local function CanAccessAll(...) return true end

---@secret-guard ... any-secret
---@param ... any
---@return boolean
local function HasAnySecret(...) return false end

-- ── Types and propagation ──────────────────────────────────────────────────────

local hp = GetHealth()
--    ^ hover: (local) hp: secret<number>
local d = definite
--    ^ hover: (local) d: secret<number>

-- Concatenation propagates.
local label = GetLabel() .. "!"
--    ^ hover: (local) label: secret<string>
local definiteText = definite .. ""
--    ^ hover: (local) definiteText: secret<string>

-- `not` on a secret boolean is a truth test: it errors, and the result is plain.
local notFlagged = not IsFlagged()
--                     ^ diag: secret-condition
--    ^ hover: (local) notFlagged: boolean

-- An operation that errors yields an ordinary value (reported once).
local doubled = hp * 2
--              ^ diag: secret-arithmetic
--    ^ hover: (local) doubled: number

-- Storing in a table value and returning it keep the secrecy.
local holder = { value = hp }
local stored = holder.value
--    ^ hover: (local) stored: secret<number>
local function Forward() return hp end
local forwarded = Forward()
--    ^ hover: (local) forwarded: secret<number>

-- `and`/`or` keep only the secrecy of an operand they return untested. A secret
-- string or number is truthy, so the ternary idiom yields plain values.
local named = GetLabel() and "named" or "unnamed"
--    ^ hover: (local) named: string
if named == "named" then end
local one = GetHealth() and 1 or 0
local incremented = one + 1
local truthy = GetLabel() and true or false
--    ^ hover: (local) truthy: true
if truthy then end

-- `type()` guards see the real type and keep the secrecy.
local mixed = GetLabel() ---@type string|number|secret<string>
if type(mixed) == "string" then
    local s = mixed
    --    ^ hover: (local) s: secret<string>
end

-- ── Guards ─────────────────────────────────────────────────────────────────────

if IsSecret(hp) then
    local inside = hp
    --    ^ hover: (local) inside: secret<number>
else
    local outside = hp
    --    ^ hover: (local) outside: number
end

if CanAccess(hp) then
    local ok = hp
    --    ^ hover: (local) ok: number
end

if not CanAccess(hp) then
    local blocked = hp
    --    ^ hover: (local) blocked: secret<number>
end

local a, b = GetHealth(), GetHealth()
if CanAccessAll(a, b) then
    local both = a + b
    --    ^ hover: (local) both: number
end

if not HasAnySecret(a, b) then
    local _ = a < b
end

local andGuarded = not IsSecret(hp) and hp > 0
local orGuarded = IsSecret(hp) or hp > 0

local record = { label = GetLabel() }
if CanAccess(record.label) then
    local _ = holder[record.label]
end

assert(CanAccess(a))
local asserted = a
--    ^ hover: (local) asserted: number
local assertedNot = GetHealth()
assert(not IsSecret(assertedNot))
local afterNotAssert = assertedNot + 1
--    ^ hover: (local) afterNotAssert: number

local function EarlyExit()
    local v = GetHealth()
    if IsSecret(v) then return end
    local after = v
    --    ^ hover: (local) after: number
    return after > 0
end

-- A local alias and the `x or function() end` polyfill keep the guard.
local aliasGuard = CanAccess
local polyfillGuard = IsSecret or function() return false end
if aliasGuard(b) then
    local _ = b >= 1
end
local c = GetHealth()
if not polyfillGuard(c) then
    local _ = -c
end

-- Checking that a guard exists (older clients lack it) doesn't hide the guard.
local function ExistenceChecked()
    local raw = GetHealth()
    if IsSecret and IsSecret(raw) then return end
    return raw > 0
end
if IsSecret and IsSecret(hp) then
    local _ = hp > 0
    --        ^ diag: secret-comparison
else
    local _ = hp > 1
end
if not IsSecret or not IsSecret(hp) then
    local _ = hp > 2
end

-- An `or` applies every field its guard proves.
local pair = { first = GetHealth(), second = GetHealth() }
local ordered = HasAnySecret(pair.first, pair.second) or (pair.first < 1 and pair.second < 1)

-- ── Diagnostics ────────────────────────────────────────────────────────────────

if hp > 0 then end
-- ^ diag: secret-comparison ~value from `GetHealth` may be secret
if hp == 0 then end
-- ^ diag: secret-comparison
-- Equality against nil (different types) doesn't error.
if hp == nil then end
-- Every relational operator errors, even against another secret.
if definite <= hp then end
-- ^ diag: secret-comparison ~value from `definite` may be secret

local sum = hp + 1
--          ^ diag: secret-arithmetic
local neg = -hp
--          ^ diag: secret-arithmetic

-- Truth tests error only for secret booleans.
if IsFlagged() then end
-- ^ diag: secret-condition ~boolean from `IsFlagged` may be secret
if hp then end
local pick = IsFlagged() and 1 or 2
--           ^ diag: secret-condition
while not IsFlagged() do end
--        ^ diag: secret-condition
-- Every operand an `and`/`or` chain tests is reported once, at its own position.
if IsFlagged()
-- ^ diag: secret-condition
    or IsFlagged()
    -- ^ diag: secret-condition
    or IsFlagged() then end
    -- ^ diag: secret-condition
local firstTwo = IsFlagged()
--               ^ diag: secret-condition
    and IsFlagged()
    --  ^ diag: secret-condition
    and IsFlagged()
local wrapped = hp and IsFlagged() or false
--                     ^ diag: secret-condition
local flagA, flagB = IsFlagged(), IsFlagged()
if flagA
-- ^ diag: secret-condition
    and flagB then end
    --  ^ diag: secret-condition
local maybeLabel = GetLabel() or "default"
--    ^ hover: (local) maybeLabel: secret<string>

-- `secret<T>?` is the optional spelling; `T|secret<T>` normalizes to `secret<T>`.
---@return secret<number>?
local function GetOptional() return nil end
local optional = GetOptional()
--    ^ hover: (local) optional: secret<number>?

local cache = {}
cache[GetLabel()] = true
--    ^ diag: secret-table-key ~value from `GetLabel` may be secret
local cached = cache[hp]
--                   ^ diag: secret-table-key
local built = { [GetLabel()] = 1 }
--               ^ diag: secret-table-key

-- Indexing a secret (string methods included), calling it, and taking its length
-- error; each reports at the secret value, and the result is an ordinary value.
local secretText = GetLabel()
local shouted = secretText:upper()
--              ^ diag: secret-access ~value from `GetLabel` may be secret; calling a method on it errors in addon code
--    ^ hover: (local) shouted: string
local firstByte = secretText[1]
--                ^ diag: secret-access ~value from `GetLabel` may be secret; indexing it errors in addon code
local lenField = secretText.len
--               ^ diag: secret-access ~indexing it
local textLength = #secretText
--                 ^ diag: secret-access ~value from `GetLabel` may be secret; taking its length errors in addon code
--    ^ hover: (local) textLength: number
local called = hp()
--             ^ diag: secret-access ~value from `GetHealth` may be secret; calling it errors in addon code
--             ^ diag: cannot-call
local chained = GetLabel():lower():upper()
--              ^ diag: secret-access ~calling a method on it
--    ^ hover: (local) chained: string
local suffixed = (secretText .. "!"):len()
--               ^ diag: secret-access ~value from `GetLabel` may be secret
local lengthBound = #GetLabel() + 1
--                  ^ diag: secret-access ~taking its length
if CanAccess(secretText) then
    local guardedUpper = secretText:upper()
    local guardedLength = #secretText
end
local guardedLower = CanAccess(secretText) and secretText:lower()
local plainUpper = ("plain"):upper()

-- A numeric `for` loop compares its counter with the limit on every iteration.
for i = 1, hp do end
--         ^ diag: secret-comparison ~value from `GetHealth` may be secret; using it as a `for` loop bound errors in addon code
for i = definite, 10 do end
--      ^ diag: secret-comparison ~value from `definite` may be secret
for i = 1, 10, GetHealth() do end
--             ^ diag: secret-comparison
for i = 1, -hp do end
--         ^ diag: secret-arithmetic
for i = 1, #secretText do end
--         ^ diag: secret-access
if CanAccess(hp) then
    for i = hp, 1, -1 do end
end

---@secret-args none
---@param text string
local function RejectsSecrets(text) end
RejectsSecrets(GetLabel())
--             ^ diag: secret-argument ~`RejectsSecrets` never accepts secret values
RejectsSecrets("plain")

---@secret-args untainted
---@param unit string
local function BlizzardOnly(unit) end
BlizzardOnly(GetLabel())
--           ^ diag: secret-argument ~`BlizzardOnly` does not accept secret values from addon code
BlizzardOnly("plain")
-- ^ doc: Secret arguments: `AllowedWhenUntainted` — accepted only from Blizzard code
local readableLabel = GetLabel()
if canaccessvalue(readableLabel) then
    BlizzardOnly(readableLabel)
end

-- No `@secret-args` at all: the argument is accepted and the result is ordinary.
---@param value any
---@return boolean
local function Inspect(value) return true end
local inspected = Inspect(hp)
--    ^ hover: (local) inspected: boolean

---@secret-args tainted
---@param n number
---@return string
local function Format(n) return "" end
local formatted = Format(hp)
--    ^ hover: (local) formatted: secret<string>

---@secret-unless unit player
---@param unit string
---@return string|secret<string>
local function NameOf(unit) return "" end
local own = NameOf("player")
--    ^ hover: (local) own: string
local other = NameOf("target")
--    ^ hover: (local) other: secret<string>

-- ── Context guards ─────────────────────────────────────────────────────────────

---@secret-clears SecretWhenScoreRestricted player
---@param player string
---@return boolean
local function ShouldScoreBeSecret(player) return true end

---@secret-restriction-guard Combat
---@return boolean
local function InLockdown() return false end

---@secret-restriction-guard kind == Enum.AddOnRestrictionState.Inactive
---@param kind Enum.AddOnRestrictionType
---@return Enum.AddOnRestrictionState
local function RestrictionState(kind) return Enum.AddOnRestrictionState.Active end

---@secret-when SecretWhenScoreRestricted
---@param player string
---@return secret<number>
local function GetScore(player) return 1 end

---@secret-when SecretWhenInCombat
---@return secret<number>
local function GetCombatTime() return 1 end

---@class ScoreCard
---@secret-when SecretWhenScoreRestricted
---@field total secret<number>

---@return ScoreCard
local function GetScoreCard() return { total = 1 } end

if not ShouldScoreBeSecret("alice") then
    local aliceScore = GetScore("alice")
    --    ^ hover: (local) aliceScore: number
    local bobScore = GetScore("bob")
    --    ^ hover: (local) bobScore: secret<number>
    local cardTotal = GetScoreCard().total
    --    ^ hover: (local) cardTotal: number
end
if not InLockdown() then
    local combatTime = GetCombatTime()
    --    ^ hover: (local) combatTime: number
end
if RestrictionState(Enum.AddOnRestrictionType.Combat) == Enum.AddOnRestrictionState.Inactive then
    local stateTime = GetCombatTime()
    --    ^ hover: (local) stateTime: number
end

---@secret-precondition RequiresScoreAccess ReturnNothing Scores need access.
---@return number?
local function GetCheckedScore() return nil end
local checkedScore = GetCheckedScore()
--                   ^ doc: Returns nothing when `RequiresScoreAccess` fails — Scores need access.

-- The failure mode is optional; a second word that isn't one starts the description.
---@secret-precondition RequiresCardAccess Cards need access.
---@return number
local function GetCheckedCard() return 1 end
local checkedCard = GetCheckedCard()
--                  ^ doc: Precondition: `RequiresCardAccess` — Cards need access.

-- A guard argument binds to the callee's parameter by name, so it reaches an API
-- that takes the same value elsewhere in its signature.
---@secret-clears SecretWhenRankRestricted season=season
---@param season number
---@return boolean
local function ShouldRankBeSecret(season) return true end

---@secret-when SecretWhenRankRestricted
---@param player string
---@param season number
---@return secret<number>
local function GetRank(player, season) return 1 end

if not ShouldRankBeSecret(11) then
    local rank = GetRank("alice", 11)
    --    ^ hover: (local) rank: number
    local oldRank = GetRank("alice", 10)
    --    ^ hover: (local) oldRank: secret<number>
end

-- A guard whose true result proves a precondition holds drops the nil a failure
-- would return.
---@secret-satisfies RequiresPair left right
---@param left string
---@param right string
---@return boolean
local function CanComparePair(left, right) return true end

---@secret-precondition RequiresPair ReturnNothing Pairs must be comparable.
---@param left string
---@param right string
---@return boolean?
local function SamePair(left, right) return true end

local uncheckedPair = SamePair("a", "b")
--    ^ hover: (local) uncheckedPair: boolean?
if CanComparePair("a", "b") then
    local checkedPair = SamePair("a", "b")
    --    ^ hover: (local) checkedPair: boolean
    local otherPair = SamePair("a", "c")
    --    ^ hover: (local) otherPair: boolean?
end

-- `@secret-args` may name the parameters it applies to.
---@secret-args untainted count
---@param text string
---@param count number
local function RepeatText(text, count) end
RepeatText(GetLabel(), 2)
RepeatText("plain", GetHealth())
--                  ^ diag: secret-argument ~`RepeatText` does not accept secret values from addon code
-- ^ doc: Secret arguments: `AllowedWhenUntainted` — accepted only from Blizzard code for `count`

-- ── Guards held in a variable ──────────────────────────────────────────────────

local storedHp = GetHealth()
local readable = CanAccess(storedHp)
if readable then
    local guardedDouble = storedHp * 2
    --    ^ hover: (local) guardedDouble: number
end
if not readable then
    local unguardedDouble = storedHp * 2
    --                      ^ diag: secret-arithmetic
end

local exitHp = GetHealth()
local exitReadable = CanAccess(exitHp)
local function UsesExitGuard()
    if not exitReadable then return end
    local scaled = exitHp * 2
    --    ^ hover: (local) scaled: number
end

-- A later write with no guard drops what the variable carried.
local staleHp = GetHealth()
local staleReadable = CanAccess(staleHp)
staleReadable = Inspect(staleHp)
if staleReadable then
    local staleDouble = staleHp * 2
    --                  ^ diag: secret-arithmetic
end

-- A write inside a branch proves nothing after the chain: the sibling branch may
-- have written something else (the compat-shim shape).
local branchHp = GetHealth()
local branchReadable
if Inspect(branchHp) then
    branchReadable = CanAccess(branchHp)
else
    branchReadable = Inspect(branchHp)
end
if branchReadable then
    local branchDouble = branchHp * 2
    --                   ^ diag: secret-arithmetic
end

-- The same in the `is-secret` direction, where trusting the branch write would
-- make a value that can never be secret look secret.
---@type number
local plainScore = 1
local scoreSecret
if Inspect(plainScore) then
    scoreSecret = IsSecret(plainScore)
else
    scoreSecret = Inspect(plainScore)
end
if scoreSecret then
    local scoreDouble = plainScore * 2
    --    ^ hover: (local) scoreDouble: number
end

-- A `local` inside a branch is scoped to it, so its guard still narrows there.
if Inspect(branchHp) then
    local innerReadable = CanAccess(branchHp)
    if innerReadable then
        local innerDouble = branchHp * 2
        --    ^ hover: (local) innerDouble: number
    end
end

-- ── A secret guard on a symbol an earlier chain operand already narrowed ───────

local chainHp = GetHealth()
if chainHp and CanAccess(chainHp) and chainHp > 0 then end
if chainHp and chainHp > 0 then end
--             ^ diag: secret-comparison
-- A `type()` guard on the same symbol keeps narrowing exactly as before.
---@type string|number
local mixedValue = 1
if mixedValue and type(mixedValue) == "string" and mixedValue:upper() then end

-- ── Annotation validation ──────────────────────────────────────────────────────

---@secret-clears
-- ^ diag: malformed-annotation
---@secret-clears SecretWhenScoreRestricted player ==
-- ^ diag: malformed-annotation
---@secret-clears * player
-- ^ diag: malformed-annotation
---@secret-clears SecretWhenScoreRestricted player=
-- ^ diag: malformed-annotation
---@secret-satisfies
-- ^ diag: malformed-annotation
---@secret-restriction-guard
-- ^ diag: malformed-annotation
---@secret-precondition
-- ^ diag: malformed-annotation
---@secret-precondition RequiresScoreAccess returnnothing Scores need access.
-- ^ diag: malformed-annotation
local function BadGuards(player) end

---@secret-args sometimes
-- ^ diag: malformed-annotation
---@secret-guard value
-- ^ diag: malformed-annotation
---@secret-unless unit
-- ^ diag: malformed-annotation
local function BadAnnotations(unit, value) end

---@return secret<number, string>
local function BadSecretArity() return 1 end
-- ^ diag: malformed-annotation ~secret<...> expects exactly one type argument
