local state = {
    cache = {},
    handler = nil,
    unused = nil,
}

local x = state.cache
state.handler = function() end
local y = state.missing
--        ^ diag: test-field-tracker ~undeclared
