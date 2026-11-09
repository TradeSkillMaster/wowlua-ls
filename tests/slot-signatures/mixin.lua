-- A plain mixin table writing function values to `self`: what other files read
-- there is the functions' signatures, not `function`.
SigProviderMixin = {}

function SigProviderMixin:OnLoad()
    self.onUpdate = function() end
    self.onDone = function(count) return count + 1 end
end

function SigProviderMixin:SetOnUpdate(callback)
    self.onUpdate = callback
end
