---@diagnostic disable: unused-local, unused-function
--- @class GcfNs
local NS = GcfNs

-- Positional / bracket / empty table literals resolve cross-file: the exhaustive
-- diagnostic check fails if any of these regress to `undefined-field`.
local _a = NS.positional
local _b = NS.bracketInt
local _c = NS.empty
local _e = NS.map
-- A field aliasing a resolved @class global resolves to that class.
local _d = NS.ref
--            ^ hover: (field) ref: GcfHelper

-- Clearing an inferred table field with nil is idiomatic, not a field-type-mismatch.
function NS:Reset()
    NS.positional = nil
    NS.empty = nil
end
