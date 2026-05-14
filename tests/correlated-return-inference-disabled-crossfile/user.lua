-- With workspace-scan synthesis disabled by `.wowluarc.json`, the cross-file
-- call does NOT pick up synthesized return-only overloads. Body-derived
-- returns still populate coarse types (any/boolean/etc.) for correct arity,
-- but there's no correlated sibling narrowing after the guard.

local function caller()
    local items, groups, count = CrossDisabledDecode("x")
    if not items then return end
    -- The cross-file external function has no `@return` annotations and (with
    -- the flag off) no synthesized overloads. Body-derived returns give us
    -- the correct arity with coarse types (`any`). Without correlated
    -- overloads, `groups`/`count` resolve to `any` — same as the enabled
    -- case (see `tests/crossfile/retoverload_synth_user.lua`), since `any`
    -- already encompasses nil and narrowing has no effect.
    local _ = groups
    --        ^ hover: (local) groups: any
    local _ = count
    --        ^ hover: (local) count: any
end
_G.CrossDisabledCaller = caller
