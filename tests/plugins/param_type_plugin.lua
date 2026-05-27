return {
    code = "test-param-type",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({init = "table"})) do
            for _, def in ipairs(var:method_defs()) do
                for _, param in ipairs(def:params()) do
                    local tn = param.type_name or "nil"
                    local nil_flag = param.nilable and "Y" or "N"
                    ctx:hint(def.range, param.name .. ":" .. tn .. ":" .. nil_flag)
                end
            end
        end
    end
}
