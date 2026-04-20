-- Cross-file global function with the synthesizable bare-return + final
-- multi-return pattern. Same shape as `tests/crossfile/retoverload_synth_lib.lua`,
-- but the adjacent `.wowluarc.json` sets `inference.correlated_return_overloads`
-- to false. The workspace-scan synthesis path must honor that flag too.
function CrossDisabledDecode(str)
    local items, groups, count = nil, nil, 0
    for i = 1, 3 do
        if i == 1 then
            if not str then
                return
            end
        elseif i == 2 then
            items = items or {}
            groups = groups or {}
        else
            return
        end
    end
    return items, groups, count
end
