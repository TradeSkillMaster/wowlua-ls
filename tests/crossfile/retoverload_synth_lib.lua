-- Cross-file global function WITHOUT `@return` annotation. Body has the
-- "bare early-out + final multi-return" shape that triggers synthesized
-- correlated return-only overloads. The call site (in `retoverload_synth_user`)
-- lives in a different file, so the function resolves through
-- PreResolvedGlobals to an external FunctionIndex — exercising the
-- workspace-scan synthesis path (annotations.rs::scan_file_globals), not
-- the per-file IR synthesis.
function CrossFileSynthDecode(str)
    local items, groups, count = nil, nil, 0
    for i = 1, 3 do
        if i == 1 then
            if not str then
                return -- bare early-out (nested inside for → if)
            end
        elseif i == 2 then
            items = items or {}
            groups = groups or {}
        else
            return -- bare early-out (nested inside for → else)
        end
    end
    return items, groups, count
end

-- Same body shape, but with hand-written `@return` annotations. The
-- annotations are authoritative — workspace-scan synthesis must NOT run on
-- top of them, otherwise sibling narrowing would invent a contract the
-- author didn't write.
---@return boolean ok
---@return string? value
function CrossFileSynthAnnotated()
    if math.random() > 0.5 then
        return true, "hi"
    end
    return false, nil
end

-- Function body ends in `while true do ... end`. The loop never falls through
-- (no escaping break) — every exit goes via the inner `return`. Synthesis must
-- NOT inject an implicit-nil tuple here. Without the `is_infinite_loop_stmt`
-- branch in `synth_block_always_exits`, the workspace-scan path would
-- spuriously emit `(any, any) | (nil, nil)` overloads even though the per-file
-- IR synthesizer (which does honor infinite loops) emits nothing — making
-- cross-file callers see narrowing that same-file callers wouldn't.
function CrossFileSynthInfinite()
    while true do
        if math.random() > 0.5 then
            return 1, 2
        end
    end
end
