# Diagnostics

wowlua-ls ships 55+ diagnostics covering type safety, annotation correctness, code quality, and WoW-specific checks. Each one is individually suppressible and configurable.

## How diagnostics work

Diagnostics run automatically as you type. They're grouped by severity:

- **Warning** — likely a bug or an annotation problem
- **Hint** — code quality suggestions, unused variables, style issues

Each diagnostic has a **code** (like `type-mismatch` or `unused-local`) that you use to suppress or configure it.

## Suppressing diagnostics

### Inline

Suppress a diagnostic on the same line by appending `disable-line` at the end:

```lua
local unused = computeSomething() ---@diagnostic disable-line: unused-local
```

When the diagnostic lands on an annotation line itself (for example `class-shadows-builtin` on a `---@class`), append the directive to that same comment line:

```lua
---@class Frame ---@diagnostic disable-line: class-shadows-builtin
---@field extra string
```

### Previous line

```lua
---@diagnostic disable-next-line: unused-local
local unused = computeSomething()
```

### Per-block

```lua
---@diagnostic disable: undefined-global
MY_GLOBAL = true
OTHER_GLOBAL = false
---@diagnostic enable: undefined-global
```

### Per-project

In `.wowluarc.json`:

```json
{
  "diagnostics": {
    "disable": ["inject-field", "unused-local"]
  }
}
```

### LuaLS compatibility aliases

For compatibility with LuaLS suppress comments, these aliases are accepted:

| Alias | Maps to |
|---|---|
| `invisible` | `access-private`, `access-protected` |
| `param-type-mismatch` | `type-mismatch` |
| `return-type-mismatch` | `return-mismatch` |

## Type safety diagnostics

These catch type errors — the most valuable diagnostics for finding real bugs.

### `type-mismatch` <Badge type="warning" text="Warning" />

Argument type doesn't match the function's `@param` annotation:

```lua
---@param name string
function greet(name) end

greet(42) -- type-mismatch: expected string, got number
```

### `return-mismatch` <Badge type="warning" text="Warning" />

Return type doesn't match the function's `@return` annotation:

```lua
---@return string
function getName()
    return 42 -- return-mismatch
end
```

### `field-type-mismatch` <Badge type="warning" text="Warning" />

Assignment to a field doesn't match its `@field` type:

```lua
---@class Config
---@field name string

---@type Config
local cfg = {}
cfg.name = 42 -- field-type-mismatch
```

### `assign-type-mismatch` <Badge type="warning" text="Warning" />

Reassignment doesn't match the variable's `@type` annotation:

```lua
---@type string
local x = "hello"
x = 42 -- assign-type-mismatch
```

### `generic-constraint-mismatch` <Badge type="warning" text="Warning" />

Generic argument doesn't satisfy the class constraint:

```lua
---@class Box<T: number|string>

---@type Box<boolean> -- generic-constraint-mismatch
local b = {}
```

The same check applies to constrained parameterized aliases (`@alias Name<T: Constraint> ...`):

```lua
---@class Frame
---@alias Wrapper<T: Frame> { value: T }

---@type Wrapper<number> -- generic-constraint-mismatch
local w = {}
```

### `need-check-nil` <Badge type="warning" text="Warning" /> <Badge type="info" text="Off by default" />

Field/method access on a possibly-nil value:

```lua
---@param name string?
function greet(name)
    print(name:upper()) -- need-check-nil
end
```

Enable in `.wowluarc.json`: `"diagnostics": { "enable": ["need-check-nil"] }`

### `grouped-return-mismatch` <Badge type="warning" text="Warning" />

Return values don't match any declared tuple-union case:

```lua
---@return (string, number) | (nil, nil)
function example()
    return "hello", nil -- grouped-return-mismatch
end
```

## Argument diagnostics

### `missing-parameter` <Badge type="warning" text="Warning" />

Required argument not provided:

```lua
---@param a number
---@param b number
function add(a, b) end

add(1) -- missing-parameter: b
```

### `redundant-parameter` <Badge type="warning" text="Warning" />

Extra arguments beyond what the function accepts:

```lua
---@param x number
function single(x) end

single(1, 2) -- redundant-parameter
```

### `cannot-call` <Badge type="warning" text="Warning" />

Calling a value whose type is known to be non-callable:

```lua
---@type table
local tbl = {}
tbl() -- cannot-call: cannot call a value of type 'table'

---@type number
local n = 5
n() -- cannot-call
```

### `invalid-op` <Badge type="warning" text="Warning" />

Arithmetic or concatenation operator applied to incompatible types. Common when `+` is used instead of `..` for string concatenation:

```lua
error("Missing context: " + tostring(field))
-- invalid-op: cannot apply '+' to 'string' and 'string' (use '..' to concatenate)

local x = true + 1
-- invalid-op: cannot apply '+' to 'boolean' and 'number'
```

## Return diagnostics

### `missing-return-value` <Badge type="warning" text="Warning" />

Return with fewer values than declared:

```lua
---@return string, number
function getInfo()
    return "hello" -- missing-return-value
end
```

### `missing-return` <Badge type="warning" text="Warning" />

Function with `@return` but no return statement on some paths:

```lua
---@return string
function getName()
    if self.name then
        return self.name
    end
    -- missing-return (falls off without returning)
end
```

### `redundant-return-value` <Badge type="warning" text="Warning" />

Returning more values than declared:

```lua
---@return string
function getName()
    return "hello", 42 -- redundant-return-value
end
```

### `implicit-nil-return` <Badge type="tip" text="Hint" /> <Badge type="info" text="Off by default" />

Bare `return` in a function with all-optional `@return` types:

```lua
---@return string?
function maybeName()
    if not self.loaded then return end -- implicit-nil-return
    return self.name
end
```

## Global and field diagnostics

### `undefined-global` <Badge type="warning" text="Warning" />

Reference to an unresolved global name:

```lua
print(MyUnknownGlobal) -- undefined-global
```

Suppress with `globals.read` in `.wowluarc.json` for known external globals. Dynamic global patterns like `_G["PREFIX" .. key] = value` are detected automatically — reads matching the prefix won't trigger this diagnostic.

### `undefined-field` <Badge type="warning" text="Warning" />

Accessing a field that doesn't exist on a `@class`:

```lua
---@class Player
---@field name string

---@type Player
local p = {}
print(p.level) -- undefined-field
```

A field read used as a **defensive existence check** is not flagged — it is
probing whether the field exists, not assuming it does. This covers the left
operand of `and`/`or`, the condition of an `if`/`while`, and the access that such
a guard protects:

```lua
local nodeID = button.GetNodeID and button:GetNodeID() -- no undefined-field
if frame.SetBackdrop then
    frame:SetBackdrop(nil) -- guarded by the condition — no undefined-field
end
local cache = obj.Custom or obj.Fallback -- the `or` fallback idiom
```

A deeper access on a now-known field is still checked, so genuine typos are not
hidden: `if obj.cfg then obj.cfg.typo end` still flags `typo`.

### `inject-field` <Badge type="tip" text="Hint" />

Setting a field not declared on a `@class`:

```lua
---@class Player
---@field name string

---@type Player
local p = {}
p.level = 10 -- inject-field
```

### `create-global` <Badge type="warning" text="Warning" />

Creating a global variable (assignment or function definition without `local`):

```lua
MyGlobal = true -- create-global
function GlobalFunc() end -- create-global
```

Suppress with `globals.write` in `.wowluarc.json`.

### `missing-fields` <Badge type="warning" text="Warning" />

Missing required fields when constructing a `@class` table:

```lua
---@class Config
---@field name string
---@field debug boolean

---@type Config
local cfg = { name = "test" } -- missing-fields: debug
```

## Annotation diagnostics

### `invalid-class-parent` <Badge type="warning" text="Warning" />

Inheriting from a primitive or literal type instead of a class:

```lua
---@class Nums : number      -- invalid-class-parent
---@class Lit : "foo"        -- invalid-class-parent
---@class Union : 1 | 2 | 3  -- invalid-class-parent
```

### `undefined-doc-class` <Badge type="warning" text="Warning" />

Undefined parent in `@class Foo : Parent`:

```lua
---@class Child : NonexistentParent -- undefined-doc-class
```

### `undefined-doc-name` <Badge type="warning" text="Warning" />

Undefined type name in any annotation:

```lua
---@param x NonexistentType -- undefined-doc-name
function foo(x) end
```

### `undefined-doc-param` <Badge type="warning" text="Warning" />

`@param` name doesn't match any function parameter:

```lua
---@param nme string -- undefined-doc-param (typo: should be 'name')
function greet(name) end
```

### `duplicate-doc-param` / `duplicate-doc-field` / `duplicate-doc-alias` <Badge type="warning" text="Warning" />

Duplicate annotation declarations.

### `circle-doc-class` <Badge type="warning" text="Warning" />

Circular inheritance chain:

```lua
---@class A : B
---@class B : A -- circle-doc-class
```

### `malformed-annotation` <Badge type="warning" text="Warning" />

Unknown or incomplete `---@` annotation.

### `unknown-diag-code` <Badge type="warning" text="Warning" />

Unknown code in `@diagnostic` directives.

## Code quality diagnostics

### `unused-local` <Badge type="tip" text="Hint" />

Unreferenced local variable.

### `unused-function` <Badge type="tip" text="Hint" /> <Badge type="warning" text="off by default" />

Unused function definition. Enable with `"diagnostics": { "enable": ["unused-function"] }` in `.wowluarc.json`.

### `redefined-local` <Badge type="tip" text="Hint" />

Same-scope local variable redefinition.

### `shadowed-local` <Badge type="tip" text="Hint" />

Local variable shadows a variable from an outer scope. Fires for `local` declarations, for-loop variables, and function parameters. Suppressed for `_`-prefixed names.

### `unreachable-code` / `code-after-break` <Badge type="tip" text="Hint" />

Code after `return` or `break`.

### `deprecated` <Badge type="warning" text="Warning" />

Usage of a symbol marked `@deprecated`.

### `discard-returns` <Badge type="warning" text="Warning" />

Ignoring the return value of a `@nodiscard` function.

### `not-precedence` <Badge type="tip" text="Hint" />

`not x < y` parses as `(not x) < y`, which is almost certainly not what you meant.

### `count-down-loop` <Badge type="warning" text="Warning" />

Numeric for-loop step direction doesn't match start/end:

```lua
for i = 10, 1 do -- count-down-loop: needs step -1
    print(i)
end
```

## Style diagnostics

### `empty-block` <Badge type="tip" text="Hint" />

Empty `if`/`while`/`for`/`repeat` body. Suppressed if the body contains a comment.

### `redundant-return` <Badge type="tip" text="Hint" />

Bare `return` as the last statement of a function's top block.

### `trailing-space` <Badge type="tip" text="Hint" />

Line ends with whitespace.

## WoW-specific diagnostics

### `wrong-flavor-api` <Badge type="warning" text="Warning" />

API call not available in all declared project flavors. See [Flavor Filtering](/guide/flavor-filtering).

### `access-private` / `access-protected` <Badge type="warning" text="Warning" />

Accessing a private or protected field from outside its visibility scope.

## Strict typing diagnostics <Badge type="tip" text="Hint" />

These are off by default. They fire when the LS can't determine a type — enable them to find gaps in your annotation coverage:

| Code | Fires when |
|---|---|
| `unknown-param-type` | Function parameter type can't be inferred |
| `unknown-return-type` | Return value has no resolvable type |
| `unknown-local-type` | `local x = expr` where expr type is unknown |
| `unknown-field-type` | Field assignment with unknown RHS type |

Enable in `.wowluarc.json`:

```json
{
  "diagnostics": {
    "enable": ["unknown-param-type", "unknown-return-type"]
  }
}
```
