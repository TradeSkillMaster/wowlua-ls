---@diagnostic disable: unused-local, unused-function

-- Regression: a frame created via the `param = param or CreateFrame(...)`
-- reassignment idiom must still resolve fields added to it, including when
-- read via `self` inside a colon method on the frame. The `or` collapses the
-- frame's type to a bare `table` (losing the class index from the symbol's
-- resolved type), so the deferred field write recovers the class from the
-- `or` operands; `self` still resolves to the named class, so the overlay
-- must land there.

-- ── Case 1: basic `frame = frame or CreateFrame(...)` + self-method read ──
local function GetBar(frame)
  frame = frame or CreateFrame("Frame")
  frame.statusBar = CreateFrame("StatusBar", nil, frame)
  frame.statusBar:SetPoint("CENTER")

  function frame:Reposition()
    self.statusBar:SetPoint("LEFT")
--       ^ hover: (field) statusBar: StatusBar {
    self.missingField = 1
    return self.missingField
--              ^ hover: (field) missingField: number
  end
  return frame
end

-- ── Case 2: version capture — a later branch reassignment (`Mixin`, an
-- `@narrows-arg` call) must not steal the field write that textually precedes
-- it. The write targets the pre-mixin frame, where `self` is also typed. ──
local CastMixin = {}
function CastMixin:OnEvent() end
local HealthMixin = {}
function HealthMixin:OnEvent() end

local function GetTypedBar(frame, kind, parent)
  frame = frame or CreateFrame("Frame", nil, parent or UIParent)

  frame.statusBar = CreateFrame("StatusBar", nil, frame)
  frame.marker = frame.statusBar:CreateTexture()
  frame.interruptMarker = CreateFrame("StatusBar", nil, frame)

  function frame:ReverseMarker()
    self.interruptMarker:SetPoint("LEFT")
--       ^ hover: (field) interruptMarker: StatusBar {
    self.statusBar:SetPoint("CENTER")
    self.marker:SetColorTexture(1, 1, 1)
  end

  if kind == "cast" then
    Mixin(frame, CastMixin)
  else
    Mixin(frame, HealthMixin)
  end

  return frame
end

-- ── Negative control: a field never assigned on the frame must still warn,
-- so the recovery does not over-suppress `undefined-field`. ──
local function GetBarStrict(frame)
  frame = frame or CreateFrame("Frame")
  frame.known = CreateFrame("StatusBar", nil, frame)

  function frame:Check()
    self.known:SetPoint("CENTER")
    return self.neverAssigned
--              ^ diag: undefined-field
  end
  return frame
end
