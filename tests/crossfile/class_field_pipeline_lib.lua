-- Comprehensive cross-file class field pipeline stress test.
-- Exercises every RHS pattern through: scanner → build_on_stubs → prescan → lookup.
-- Each field uses a DIFFERENT RHS pattern to catch regressions in any branch.
-- Requires: --with-stubs

---@class PipelineWidget : Frame
local W = CreateFrame("Frame", "PipelineTestFrame", UIParent)

-- Pattern 1: literal number
W.litNumber = 42

-- Pattern 2: literal string
W.litString = "hello"

-- Pattern 3: literal boolean
W.litBool = true

-- Pattern 4: function literal
W.litFunc = function(self) return self end

-- Pattern 5: table constructor
W.litTable = { x = 1, y = 2 }

-- Pattern 6: direct method call (colon)
W.directMethodCall = W:CreateTexture(nil, "BACKGROUND")

-- Pattern 7: direct global function call
W.directGlobalCall = CreateFrame("Frame", nil, W)

-- Pattern 8: indirect via local from method call (the pattern that broke)
local tex1 = W:CreateTexture(nil, "OVERLAY")
W.indirectMethodLocal = tex1

-- Pattern 9: indirect via local from global call
local subFrame = CreateFrame("Frame", nil, W)
W.indirectGlobalLocal = subFrame

-- Pattern 10: indirect via local from chained method call
local fontStr = W:CreateFontString(nil, "OVERLAY")
W.indirectFontString = fontStr

-- Pattern 11: nested sub-table field
local inner = CreateFrame("Frame", nil, W)
W.SubPanel = inner

local innerTex = inner:CreateTexture(nil, "BACKGROUND")
W.SubPanel.InnerTexture = innerTex

-- ── Inject-field contract interaction ──────────────────────────────
-- @class-annotated variables are class definitions — inject-field does not fire
-- even when @field annotations exist (the variable IS the class, not an instance).

---@class PipelineContract : Frame
---@field declared Texture
local C = CreateFrame("Frame", "PipelineContractFrame", UIParent)
--    ^ hover: (local) C: PipelineContract {

local declaredTex = C:CreateTexture(nil, "OVERLAY")
C.declared = declaredTex

local undeclaredTex = C:CreateTexture(nil, "BACKGROUND")
C.undeclared = undeclaredTex
