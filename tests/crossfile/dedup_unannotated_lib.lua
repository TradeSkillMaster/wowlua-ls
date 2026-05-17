-- Duplicate definitions without annotations (simulates FrameXML source stubs)
-- These have bodies with return statements (body-derived returns → "any")
function MixinA:IsValid()
    return self.valid
end

function MixinA:GetCount(name)
    return #self.items
end
