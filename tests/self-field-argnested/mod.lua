-- Regression: a `self.field = <call>` whose ARGUMENT nests another call — e.g.
-- `select(3, UnitClass("player"))` — is NOT a chained receiver: the outer callee
-- (`select`) is still a plain resolvable name. `funcall_has_chained_receiver`
-- used to exclude only `SyntaxKind::ExpressionList`, but call arguments parse as
-- `SyntaxKind::ArgumentList`, so the nested arg-call made the assignment look
-- chained — it was parked by the bare scanner as an existence-only `any`
-- placeholder instead of being routed to the funcall scanner. The fix excludes
-- the argument list (whichever kind it parses as), so the funcall scanner owns
-- it and the field resolves to its precise scalar type instead of `any`.
--
-- A genuinely-chained receiver (`Make():Build()`, a call on a call's result)
-- is still detected as chained and parked by the bare scanner as an
-- existence-only `any` placeholder — the disjoint-coverage boundary
-- `funcall_has_chained_receiver` draws. But the query-time resolver refines
-- that `any` placeholder from the in-file assignment when it's resolvable, so
-- hover/completion show the concrete type (matching the diagnostics engine).
---@diagnostic disable: unused-local

---@class ArgNestHost
local Host = {}

function Host:Build() return self end

---@return ArgNestHost
local function Make() return Host end

function Host:Setup()
    -- Arg-nested funcalls (the nested call sits in the ARGUMENT list, not the
    -- receiver). `UnitClass` returns className, classFilename, classID; `select`
    -- projects one — so these resolve to scalars, not bare `table`.
    self.classId = select(3, UnitClass("player"))
    self.classTag = select(2, UnitClass("player"))
    -- Genuinely chained (a call on a call's result): the bare scanner parks
    -- this as an existence-only `any` placeholder (the coarse scan can't
    -- resolve the chain), but the query-time resolver refines it from this
    -- write — Make() -> ArgNestHost, :Build() -> self -> ArgNestHost.
    self.chained = Make():Build()
end

function Host:Use()
    local a = self.classId
    --              ^ hover: (field) classId: number
    local b = self.classTag
    --              ^ hover: (field) classTag: string
    local c = self.chained
    --              ^ hover: (field) chained: ArgNestHost {
    return a, b, c
end
