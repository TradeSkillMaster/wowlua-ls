# Secret Values

Starting with retail 12.0, many WoW APIs return **secret values** while addon restrictions are active (combat, encounters, Mythic+, PvP, and so on). Addon code may store a secret, pass it around, and hand it to a widget (`bar:SetValue(hp)`), but comparing it, doing arithmetic on it, or using it as a table key is an immediate Lua error. These errors only happen under restrictions, so they are easy to miss in testing. wowlua-ls tracks which values may be secret and reports those operations as you type.

## Secret types

A value that may be secret has the type `secret<T>`:

```lua
local hp = UnitHealth("target")         -- secret<number>
local name = UnitName("target")         -- secret<string>
local myName = UnitName("player")       -- string: the player's identity is never secret
```

Secrecy follows the value: storing it in a local or a table field, returning it from a function, and concatenating it all keep the `secret<T>`. There is no separate "always secret" type: every secret-producing API depends on game state, so `secret<T>` is the only secrecy there is (a hand-written `T|secret<T>` means the same thing).

Hovering an API shows a **Secrecy** section with Blizzard's documentation of when it returns secrets:

```
function UnitClass(unit: UnitToken)
  -> className: secret<string>, classFilename: secret<string>, classID: secret<number>

Secrecy
- May return secret values: `SecretWhenUnitIdentityRestricted` — Guarded APIs and events produce secret values when the unit isn't player-controlled or in the party/raid. …
- Never secret when `unit` is `"player"` or `"pet"`
```

## What is reported

| Operation on a possibly-secret value | In addon code | Diagnostic |
|---|---|---|
| Arithmetic: `+ - * / % ^`, unary `-` | Error | `secret-arithmetic` |
| Ordering: `< > <= >=` | Error | `secret-comparison` |
| Start, limit, or step of a numeric `for` loop (`for i = 1, hp do`)\* | Error | `secret-comparison` |
| `==` / `~=` with a value that may have the same type | Error | `secret-comparison` |
| `==` / `~=` with `nil` or a value of another type | Allowed | |
| Testing a secret **boolean** (`if`, `while`, `until`, `not`, left side of `and` / `or`) | Error | `secret-condition` |
| Testing any other secret (`if name then`) | Allowed | |
| Table key: `t[secret] = v`, `{ [secret] = v }`, `t[secret]`\* | Error | `secret-table-key` |
| Argument to an API that never accepts secrets (e.g. `C_ChatInfo.SendAddonMessage`) | Error | `secret-argument` |
| Length `#`, indexing (`name:upper()`, `name.x`, `name[1]`), or calling a secret | Error | `secret-access` |
| `..`, `string.format`, `string.join`, `string.concat` | Allowed; the result is secret | |
| `a and b`, `a or b` | Allowed; the result is secret only if the operand it returns is (`UnitName(unit) and "named" or "unnamed"` is plain) | |
| `type(secret)` | Allowed; returns the real type | |

Rules marked \* are assumed; the rest are documented by Blizzard. Other APIs documented to accept secrets from addon code are also assumed to return secret results when given secrets.

An operation that errors is reported once: its result is treated as an ordinary value, so later uses of it aren't flagged again.

```lua
local hp, maxHp = UnitHealth("target"), UnitHealthMax("target")
local pct = hp / maxHp      -- secret-arithmetic
if hp < maxHp then end      -- secret-comparison
if UnitIsAFK("target") then end  -- secret-condition
local short = UnitName("target"):sub(1, 3)  -- secret-access
```

## Guarding

The builtins `issecretvalue`, `canaccessvalue`, `canaccessallvalues`, and `hasanysecretvalues` narrow their arguments:

```lua
local hp, maxHp = UnitHealth("target"), UnitHealthMax("target")

if canaccessallvalues(hp, maxHp) then
    local pct = hp / maxHp     -- hp, maxHp: number
end

if issecretvalue(hp) then return end
local low = hp < 1000          -- hp: number for the rest of the scope
```

Narrowing works with `not`, `elseif`, `and` / `or`, `assert`, early exits, field chains (`self.health`), and local aliases. For addons that also load on older clients, it sees through the existence check `if issecretvalue and issecretvalue(hp) then` and the `local issecret = issecretvalue or function() return false end` polyfill.

### Custom guards

Mark your own guard function with `@secret-guard <param> <kind>` so calls to it narrow the same way:

```lua
---@secret-guard value accessible
---@param value any
---@return boolean
function MyAddon:IsReadable(value)
    return canaccessvalue(value)
end
```

| Kind | `true` means | `false` means |
|---|---|---|
| `is-secret` | the argument is secret | the argument is not secret |
| `accessible` | no argument is secret | the argument is secret (one argument only) |
| `any-secret` | the argument is secret (one argument only) | no argument is secret |

Use `...` as the parameter name for a vararg guard (`@secret-guard ... accessible`).

## Guarding by context

Blizzard also provides guards that test the *situation* instead of a value: `C_Secrets.Should*BeSecret` asks whether a query would return secrets right now, and `C_RestrictedActions.IsAddOnRestrictionActive` asks whether a restriction is in effect. Where such a guard proves the relevant secrecy doesn't apply, the APIs and structure fields it covers return ordinary values:

```lua
if not C_Secrets.ShouldUnitHealthMaxBeSecret(unit) then
    local max = UnitHealthMax(unit)       -- number
    bar:SetMinMaxValues(0, max)
end

if C_Secrets.ShouldAurasBeSecret() then return end
local aura = C_UnitAuras.GetAuraDataByIndex(unit, 1, "HARMFUL")
if aura and aura.duration > 5 then end    -- aura.duration: number

if not InCombatLockdown() then
    local session = C_DamageMeter.GetCombatSessionFromType(sessionType, meterType)  -- fields: plain
end
```

Guards work in either branch of `if`/`elseif`/`else`, with `not`, on the right side of `and`/`or`, before early exits, in `assert` and `while`, and behind the `C_Secrets and C_Secrets.ShouldAurasBeSecret()` check older clients need. `C_RestrictedActions.GetAddOnRestrictionState(type) == Enum.AddOnRestrictionState.Inactive` counts as an inactive restriction (`Activating` counts as active). Write the restriction as `Enum.AddOnRestrictionType.Combat` or through a local alias of the enum.

A guard covers the code it guards in the same function: an API result fetched before the guard stays secret (fetch it inside), and a function defined inside the guard runs later, so its body isn't covered. Structure fields are the exception — they clear wherever they are *read*, however the structure was obtained. Guards about a unit, spell, or slot clear only later calls passing the same local, field (`self.unit`), or literal in that position; guards about a pair of units need both to match.

| Guard (`C_Secrets.`) | Clears |
|---|---|
| `HasSecretRestrictions()` | Every secret, including values fetched before the guard |
| `ShouldAurasBeSecret()` | Aura data |
| `ShouldUnitAuraIndexBeSecret(unit, index)`, `ShouldUnitAuraInstanceBeSecret(unit, auraInstanceID)`, `ShouldUnitAuraSlotBeSecret(unit, slot)` | Aura data for the same unit and index, instance, or slot |
| `ShouldSpellAuraBeSecret(spell)`, `GetSpellAuraSecrecy(spell) == Enum.SecrecyLevel.NeverSecret` | Aura data for the same spell |
| `ShouldCooldownsBeSecret()` | Cooldowns |
| `ShouldSpellCooldownBeSecret(spell)`, `GetSpellCooldownSecrecy(spell) == Enum.SecrecyLevel.NeverSecret`, `ShouldActionCooldownBeSecret(action)`, `ShouldSpellBookItemCooldownBeSecret(slot, bank)` | Cooldowns for the same spell, action, or spellbook item |
| `ShouldTotemSlotBeSecret(slot)` | Totem information for the same slot |
| `ShouldUnitHealthMaxBeSecret(unit)` | Maximum health for the same unit |
| `ShouldUnitIdentityBeSecret(unit)` | Identity and name for the same unit |
| `ShouldUnitPowerBeSecret(unit, powerType)`, `ShouldUnitPowerMaxBeSecret(unit, powerType)` | Power or maximum power for the same unit and power type |
| `ShouldUnitSpellCastBeSecret(unit, spell)`, `ShouldUnitSpellCastingBeSecret(unit)` | Cast information for the same unit |
| `ShouldUnitStatsBeSecret()` | Stats |
| `ShouldUnitComparisonBeSecret(unit1, unit2)` | `UnitIsUnit` for the same units |
| `ShouldUnitThreatStateBeSecret(unit, mobUnit)`, `ShouldUnitThreatValuesBeSecret(unit, mobUnit)` | Threat state or values for the same units |

An inactive restriction (`IsAddOnRestrictionActive`, `GetAddOnRestrictionState`, or `InCombatLockdown` for combat) clears the secrecy that depends only on restrictions, once every restriction it depends on is inactive:

| Secrecy | Restrictions |
|---|---|
| `SecretWhenInCombat` | Combat |
| Auras, cooldowns, and totems (`SecretWhenAurasRestricted`, `SecretWhenUnitAuraRestricted`, `SecretWhenCooldownsRestricted`, `SecretWhenTotemSlotSecret`) | Combat, Encounter, ChallengeMode, PvPMatch |
| `SecretInChatMessagingLockdown` | Encounter, ChallengeMode, PvPMatch, Chat |
| `SecretOnRestrictedMaps`, `SecretWhenUnitComparisonRestricted` | Map |
| `SecretInActivePvPMatch` | PvPMatch |
| `SecretWhenEncounterEvent` | Encounter |

Secrecy that depends on the unit or object queried (identity, health, power, stats, casts, threat, anchoring, …) is cleared only by its own `C_Secrets` guard.

Limitations:

- Structure fields (`AuraData.duration`) clear by what the structure depends on, not by which unit, spell, or aura it describes — nor by when it was obtained, so a structure cached from an earlier frame reads as plain inside a guard.
- Blizzard lets individual spells be flagged "always secret", overriding restrictions; restriction guards can't see these flags.
- Assumed, not documented: `InCombatLockdown()` is the `Combat` restriction, `SecretWhenEncounterEvent` depends on the `Encounter` restriction, and the communication-restricted maps of `SecretInChatMessagingLockdown` are the `Chat` restriction.
- `IsAddOnRestrictionActive` always returns `false` while `ADDON_RESTRICTION_STATE_CHANGED` is being dispatched.
- A guard stored in a variable (`local secretAuras = C_Secrets.ShouldAurasBeSecret()`) isn't recognized.

Mark your own context guards with [`@secret-clears` and `@secret-restriction-guard`](/reference/annotations#secret-value-annotations).

## Where secrecy comes from

The retail API stubs are generated from Blizzard's API documentation, which marks the functions, event payloads, and structure fields that may be secret, the conditions under which they are, and the APIs that reject secret arguments. A few details:

- **Player exemptions.** Unit APIs whose restriction never applies to the player (`UnitName`, `UnitClass`, `UnitCastingInfo`, `UnitPowerMax`, …) return ordinary values for a literal `"player"` (and `"pet"` where documented). The stubs express this with `@secret-unless`.
- **Structures the documentation doesn't describe.** Blizzard's documentation returns `AuraData` without listing its fields, so their secrecy comes from warcraft.wiki.gg: every field may be secret except the ones it marks as never secret (`auraInstanceID`, `isHarmful`, `isHelpful`, …).
- **Constant accessors.** Some object methods accept secret arguments without marking their object secret, and return secrets exactly when an argument is secret (`formatter:Format(secretSeconds)` is `secret<string>`, `curve:Evaluate(0.5)` is `number`).
- **Preconditions.** Some APIs check a precondition first and return nothing (`UnitIsUnit` with units that can't be compared) or raise an error (the `C_UnitAuras` family without aura access) when it fails. Hover lists them, and the returns of the ones that return nothing are nilable.
- **Widgets.** Passing a secret to a widget setter is allowed. Blizzard marks widgets that received secrets as having a secret *aspect*, after which their getters return secrets. Hover shows these aspects, but getters aren't treated as secret, since most widgets never receive secrets.
- **Your own code.** Returns and fields assigned from secret values are inferred automatically. To declare secrecy explicitly (for example on a function whose body isn't visible), write `secret<T>` in the type and use the annotations in the [reference](/reference/annotations#secret-value-annotations).

## Flavors

Secret values only exist on retail, so the checks follow the same flavor signals as [flavor filtering](/guide/flavor-filtering):

- **Declared flavors.** A `.wowluarc.json` `flavors` list, or the `.toc` files (`## Interface:` versions, `_Vanilla`/`_Classic` suffixes, `AllowLoadGameType`). A file that never loads on retail sees plain types, no **Secrecy** hover section, and no `secret-*` diagnostics. No flavor signal at all counts as retail.
- **Guards.** Nothing is reported inside `if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then`, inside `if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then`, after an early exit like `if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then return end`, under a [`@flavor-narrows`](/guide/flavor-filtering#conditional-narrowing) guard or a boolean initialized from a `WOW_PROJECT_ID` comparison (`local isClassic = WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE`), or on the right side of an `and` whose left side is such a guard (`if isClassic and hp > 0 then`). This works with or without a `flavors` declaration, and hover, inlay hints, completion, and signature help hide secrecy there too.

## Testing

Every restriction can be forced from anywhere, so each report can be reproduced without entering combat or an instance. Set a console variable with `/console` (`/console addonCombatRestrictionsForced 1`); none of them are saved across restarts:

| Console variable | Restriction |
|---|---|
| `addonCombatRestrictionsForced` | Combat |
| `addonEncounterRestrictionsForced` | Encounter |
| `addonChallengeModeRestrictionsForced` | ChallengeMode |
| `addonPvPMatchRestrictionsForced` | PvPMatch |
| `addonMapRestrictionsForced` | Map |
| `addonChatRestrictionsForced` | Chat |

`secretwrap(...)` returns its arguments as secrets, to feed an operation a secret directly (`local hp = secretwrap(100)`).

## Limitations

- A guard helper without `@secret-guard` (or `@secret-clears` / `@secret-restriction-guard`) isn't recognized. Annotate it.
- A secret stored through a local or an expression (`ns.label = name .. "!"`) and read in another file loses its secrecy.
- A widget that received a secret doesn't make its getters secret yet.
- Some restrictions depend on game state the language server can't see (a spell cast by the player, for instance), so a few reports are conservative. Suppress those with `---@diagnostic disable-next-line: secret-comparison` (or the relevant code).
