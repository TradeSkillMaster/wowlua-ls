-- With workspace-scan synthesis disabled by `.wowluarc.json`, the cross-file
-- call does NOT pick up synthesized return-only overloads. Body-derived
-- returns still populate coarse types (any/boolean/etc.) for correct arity,
-- but there's no correlated sibling narrowing after the guard.

local function caller()
    local items, groups, count = CrossDisabledDecode("x")
    if not items then return end
    -- The cross-file external function has no `@return` annotations and (with
    -- the flag off) no synthesized overloads, so there is no correlated sibling
    -- narrowing after the guard. `groups` (an anonymous `{}` table or nil) lifts
    -- to `any`; the precise body-derived `count` (the literal `0`) resolves to
    -- `number`. Neither is affected by the absent overloads.
    local _ = groups
    --        ^ hover: (local) groups: any
    local _ = count
    --        ^ hover: (local) count: number
end
_G.CrossDisabledCaller = caller
