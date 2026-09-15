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
| `==` / `~=` with a value that may have the same type | Error | `secret-comparison` |
| `==` / `~=` with `nil` or a value of another type | Allowed | |
| Testing a secret **boolean** (`if`, `while`, `until`, `not`, left side of `and` / `or`) | Error | `secret-condition` |
| Testing any other secret (`if name then`) | Allowed | |
| Table key: `t[secret] = v`, `{ [secret] = v }`, `t[secret]`\* | Error | `secret-table-key` |
| Argument to an API that never accepts secrets (e.g. `C_ChatInfo.SendAddonMessage`) | Error | `secret-argument` |
| Length `#`, indexing, or calling a secret | Error | not reported yet |
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

## Where secrecy comes from

The retail API stubs are generated from Blizzard's API documentation, which marks the functions, event payloads, and structure fields that may be secret, the conditions under which they are, and the APIs that reject secret arguments. A few details:

- **Player exemptions.** Unit APIs whose restriction never applies to the player (`UnitName`, `UnitClass`, `UnitRace`, …) return ordinary values for a literal `"player"` (and `"pet"` where documented). The stubs express this with `@secret-unless`.
- **Widgets.** Passing a secret to a widget setter is allowed. Blizzard marks widgets that received secrets as having a secret *aspect*, after which their getters return secrets. Hover shows these aspects, but getters aren't treated as secret, since most widgets never receive secrets.
- **Your own code.** Returns and fields assigned from secret values are inferred automatically. To declare secrecy explicitly (for example on a function whose body isn't visible), write `secret<T>` in the type and use the annotations in the [reference](/reference/annotations#secret-value-annotations).

## Flavors

Secret values only exist on retail, so the checks follow the same flavor signals as [flavor filtering](/guide/flavor-filtering):

- **Declared flavors.** A `.wowluarc.json` `flavors` list, or the `.toc` files (`## Interface:` versions, `_Vanilla`/`_Classic` suffixes, `AllowLoadGameType`). A file that never loads on retail sees plain types, no **Secrecy** hover section, and no `secret-*` diagnostics. No flavor signal at all counts as retail.
- **Guards.** Nothing is reported inside `if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then`, inside `if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then`, after an early exit like `if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then return end`, under a [`@flavor-narrows`](/guide/flavor-filtering#conditional-narrowing) guard or a boolean initialized from a `WOW_PROJECT_ID` comparison (`local isClassic = WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE`), or on the right side of an `and` whose left side is such a guard (`if isClassic and hp > 0 then`). This works with or without a `flavors` declaration, and hover, inlay hints, completion, and signature help hide secrecy there too.

## Limitations

- A guard helper without `@secret-guard` isn't recognized. Annotate it.
- A secret stored through a local or an expression (`ns.label = name .. "!"`) and read in another file loses its secrecy.
- A widget that received a secret doesn't make its getters secret yet.
- Some restrictions depend on game state the language server can't see (a spell cast by the player, for instance), so a few reports are conservative. Suppress those with `---@diagnostic disable-next-line: secret-comparison` (or the relevant code).
