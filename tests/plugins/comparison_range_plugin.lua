return {
    code = "test-comparison-range",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({ name = "private", init = "table" })) do
            for _, def in ipairs(var:method_defs()) do
                for _, param in ipairs(def:params()) do
                    for _, c in ipairs(param:comparisons()) do
                        if type(c.literal) == "string" then
                            -- Encode the range so the test can assert it points at
                            -- the comparison, not the param's definition site.
                            ctx:warn(c.range, c.literal .. ":" .. c.range.start .. ":" .. c.range["end"])
                        end
                    end
                end
            end
        end
    end
}
