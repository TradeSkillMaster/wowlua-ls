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

---@secret-args none
---@param text string
local function RejectsSecrets(text) end
RejectsSecrets(GetLabel())
--             ^ diag: secret-argument ~`RejectsSecrets` never accepts secret values
RejectsSecrets("plain")

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

-- ── Annotation validation ──────────────────────────────────────────────────────

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
