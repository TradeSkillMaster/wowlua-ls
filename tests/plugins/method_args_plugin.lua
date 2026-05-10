return {
    code = "test-method-args",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({init = "table"})) do
            for _, call in ipairs(var:method_calls()) do
                for _, arg in ipairs(call:args()) do
                    if arg.kind == "string" and arg.literal then
                        ctx:warn(arg.range, "string arg: " .. arg.literal)
                    end
                end
            end
        end
    end
}
