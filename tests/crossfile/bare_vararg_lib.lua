-- Cross-file bare vararg: a method whose `...` has no `@param ...` type.
-- The workspace scan synthesizes an empty annotation for the bare `...`; it must
-- be treated as absent so the hover renders `...`, not `...: ` (dangling colon).

---@class BareVarargMixin
local BareVarargMixin = {}

function BareVarargMixin:Collect(first, ...) end
