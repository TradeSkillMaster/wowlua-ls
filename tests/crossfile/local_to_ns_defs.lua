-- Cross-file test: local variable assigned from function call, then assigned to namespace field
local addonName, ns = ...

-- Create a parent frame (CreateFrame returns a known type)
local parent = CreateFrame("Frame")

-- Pattern 1: local assigned from method call, then assigned to ns field
local surgeArc = parent:CreateTexture(nil, "OVERLAY", nil, 1)
ns.SurgeArc = surgeArc

-- Pattern 2: local assigned from global function call, then assigned to ns field
local textDisplay = CreateFrame("Frame", nil, parent)
ns.TextDisplay = textDisplay

-- Pattern 3: chained local — method call on a local that itself came from a call
local textBackground = textDisplay:CreateTexture(nil, "BACKGROUND")
ns.TextBackground = textBackground

-- Pattern 4: local with a non-Simple @type annotation (union) → ns field.
-- Must carry the full union cross-file, not degrade to `any`.
---@type number | string
local idOrName = 1
ns.L2N_IdOrName = idOrName

-- Pattern 5: local with a `table<K,V>` @type annotation → ns field.
-- Must carry the map type, not degrade to a bare `table`.
---@type table<string, string>
local labels = {}
ns.L2N_Labels = labels

-- Pattern 6: local with a plain scalar-literal value (no annotation) → ns field.
-- Must infer `number` from the literal, not degrade to `any`.
local retryCount = 3
ns.L2N_RetryCount = retryCount

-- Pattern 7: precedence — an explicit `@type` on a call-assigned local must
-- override the call's inferred same-file `@return`, matching LuaLS. `PrecBuilder`
-- is a non-local method so its return feeds `local_return_types`; the compound
-- `@type` (which can't fit the Simple-only fast path) must still win.
-- (Field names are prefixed to avoid colliding with other tests that share the
-- workspace-wide addon namespace, per TFT_/NDF_/FNF_ elsewhere in this dir.)
local PrecBuilder = {}
---@return Frame
function PrecBuilder.make() return CreateFrame("Frame") end

---@type Frame | Texture
local typedWidget = PrecBuilder.make()
ns.L2N_TypedWidget = typedWidget

-- Control: same call, no `@type` → the inferred `@return` (Frame) is used.
local inferredWidget = PrecBuilder.make()
ns.L2N_InferredWidget = inferredWidget
