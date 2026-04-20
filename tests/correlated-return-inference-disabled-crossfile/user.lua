-- With workspace-scan synthesis disabled by `.wowluarc.json`, the cross-file
-- call does NOT pick up synthesized return-only overloads. The base return
-- type for slot 1+ is recovered from the function's `func.rets` union, so
-- siblings stay optional even after the success-guard narrows `items`.

local function caller()
    local items, groups, count = CrossDisabledDecode("x")
    if not items then return end
    -- The cross-file external function has no `@return` annotations and (with
    -- the flag off) no synthesized overloads, so the per-slot resolver has no
    -- type information for siblings — they stay `?`. The point of the test:
    -- with synthesis ENABLED, `groups`/`count` would resolve to `any` after
    -- sibling narrowing (see `tests/crossfile/retoverload_synth_user.lua`).
    -- Showing `?` here proves the workspace-scan synthesis path didn't fire.
    local _ = groups
    --        ^ hover: (local) groups: ?
    local _ = count
    --        ^ hover: (local) count: ?
end
_G.CrossDisabledCaller = caller
