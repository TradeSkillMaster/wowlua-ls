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

-- A source table whose `.func` field holds a callable, forwarded below.
local source = { func = getName }

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

-- Forwarded-value fields: a namespace field assigned from another *field* or a
-- *parameter* inside a function body. The coarse scan can't see the forwarded
-- value's type, but it may be a callable, so these register callable-or-unknown
-- (`function & table`) rather than a bare non-callable `table` — calling them
-- cross-file must not false-positive as `cannot-call`. (A function *call* RHS
-- like `makeWidget()` above instead stays a bare `table`: its result is more
-- often a frame/table than a callable, and the bare type lets a competing
-- concrete type subsume it in a union.)
function addonTable.Wire(handler)
    -- forwarded from a parameter
    addonTable.OnClick = handler
    -- forwarded from another field
    addonTable.GetValue = source.func
    -- forwarded from a parameter onto a deep namespace path
    addonTable.Sub.Run = handler
    -- a function *literal* is callable too (typed `function`, not the
    -- forwarded callable-or-unknown intersection)
    addonTable.Render = function() end
end
