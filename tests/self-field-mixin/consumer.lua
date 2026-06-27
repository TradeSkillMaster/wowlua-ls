---@diagnostic disable: unused-local
-- Another method on the same mixin, in a different file. `self` is the
-- XML-promoted DataProviderMixin class, so these reads are checked. The
-- self-fields written in provider.lua must all resolve here:
--   - `provider` / `handle`: existence-only (bare `table`), no undefined-field
--   - `onUpdate`: registered callable (a function literal), so calling it is
--     not a `cannot-call` error
-- Note `provider` is forwarded from a *parameter* (`self.provider = provider`)
-- yet stays a bare `table`, NOT the `function & table` callable-or-unknown that
-- a forwarded *namespace/@class* field gets: that treatment is deliberately
-- scoped to non-`self` roots (self data-fields are usually data, and forcing
-- them callable would union with their real per-file type and turn clean reads
-- into `type-mismatch`). If this starts asserting `function & table`, that
-- scoping has regressed.
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
