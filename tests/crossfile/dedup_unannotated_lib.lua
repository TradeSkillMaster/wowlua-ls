-- Duplicate definitions without annotations (simulates FrameXML source stubs)
-- These have bodies with return statements (body-derived returns → "any")
function MixinA:IsValid()
    return self.valid
end

function MixinA:GetCount(name)
    return #self.items
end

-- Duplicate with body ending in `return nil` (body-derived returns → "nil")
-- Should NOT create a spurious `-> nil` overload
function MixinA:GetId()
    if self.backing then
        return 123
    end
    return nil
end
