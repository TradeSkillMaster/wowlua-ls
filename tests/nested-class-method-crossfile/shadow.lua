-- Regression: a NON-`@class` local re-declaration shadowing an enclosing `@class`-typed
-- name must drop that name from scope, so methods defined on the shadowing local are not
-- mis-attributed to the outer class (which would mask a real `undefined-field` and
-- pollute the class's completions). Mirrors the parameter-shadowing case.
---@diagnostic disable: unused-local, unused-function, redefined-local, shadowed-local

--- @class NCM_Shadow
local Shadow = {}

--- @type table<Frame, NCM_Shadow>
Shadow.items = {}

function Shadow:Build()
    local Shadow = CreateFrame("Frame")
    -- `Shadow` above shadows the outer `@class NCM_Shadow`; `FrameOnly` is a method on
    -- that Frame local, so it must NOT be attributed to NCM_Shadow.
    function Shadow:FrameOnly() end
end

function Shadow:Use(frame)
    -- Calling `FrameOnly` on a real NCM_Shadow instance is therefore a genuine
    -- undefined-field (it would be masked if the shadowing leaked the method).
    self.items[frame]:FrameOnly()
    --                ^ diag: undefined-field
end
