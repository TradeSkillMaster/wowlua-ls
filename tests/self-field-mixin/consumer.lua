---@diagnostic disable: unused-local
-- Another method on the same mixin, in a different file. `self` is the
-- XML-promoted DataProviderMixin class, so these reads are checked. The
-- self-fields written in provider.lua must all resolve here:
--   - `provider` / `handle`: existence-only (bare `table`), no undefined-field
--   - `onUpdate`: registered callable (a function literal), so calling it is
--     not a `cannot-call` error
function DataProviderMixin:Render()
    local p = self.provider
    --              ^ hover: (field) provider: table
    self.onUpdate()
    local h = self.handle
    --              ^ hover: (field) handle: table
    local n = self.viaNestedFn
    --              ^ hover: (field) viaNestedFn: table
    return p, h, n
end
