return {
    code = "test-dot-syntax",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({init = "table"})) do
            local defs = var:method_defs()
            local calls = var:method_calls()
            -- Report each def
            for _, def in ipairs(defs) do
                ctx:hint(def.range, "def: " .. def.method_name)
            end
            -- Report each call
            for _, call in ipairs(calls) do
                ctx:hint(call.range, "call: " .. call.method_name)
            end
        end
    end
}
