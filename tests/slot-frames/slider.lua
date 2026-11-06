-- A mixin global an author declares a class of another name, which an XML
-- template names as its mixin.
--- @class LedgerLevelingSlider: Slider, MinimalSliderWithSteppersTemplate
LedgerLevelingSliderMixin = CreateFromMixins(MinimalSliderWithSteppersMixin)
--- @class LedgerLevelingSlider
local LevelingSlider = LedgerLevelingSliderMixin
LevelingSlider:GenerateCallbackEvents({ "OnDragStop" })
function LevelingSlider:OnLoad() self:SetValue(1) end
