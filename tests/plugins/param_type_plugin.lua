return {
    code = "test-param-type",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({init = "table"})) do
            for _, def in ipairs(var:method_defs()) do
                for _, param in ipairs(def:params()) do
                    local tn = param.type_name or "nil"
                    ctx:hint(def.range, param.name .. ":" .. tn)
                end
            end
        end
    end
}
