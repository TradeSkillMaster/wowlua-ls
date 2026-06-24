-- Cross-file: namespace fields assigned only from INSIDE function bodies must
-- still register as fields, so reads in other files don't false-positive as
-- `undefined-field`. The coarse cross-file scan deliberately does not *type*
-- values produced inside functions (it can't see runtime mixins, local frames,
-- etc.), so these fields are registered existence-only (a bare `table`); that
-- suppresses `undefined-field` on reads without fabricating a wrong concrete
-- type that would cause `field-type-mismatch` on the write or spurious
-- `undefined-field` on the field's own sub-accesses.
---@class InFuncNS
local addonTable = select(2, ...)

local function getName()
    return "abc"
end

-- Builds a value whose real (mixin'd) shape the coarse scan cannot see.
local function makeWidget()
    local w = {}
    function w:Ping()
        return true
    end
    return w
end

-- top-level multi-target assignment (not inside any function): exercises the
-- `!in_function && idents.len() >= 2` branch of the existence-only scan
addonTable.MinX, addonTable.MaxX = 0, 100

function addonTable.Setup()
    -- single-target field write inside a function
    addonTable.Title = getName()
    -- multi-target field write inside a function (neither target is scanned by
    -- the single-target main loop)
    addonTable.Width, addonTable.Height = 10, 20
    -- deep-chain field write inside a function
    addonTable.Sub = {}
    addonTable.Sub.Value = 5
    -- complex value assigned inside a function: must NOT be typed precisely
    -- (otherwise its mixin'd `:Ping` would wrongly report undefined-field)
    addonTable.InFuncWidget = makeWidget()
end
