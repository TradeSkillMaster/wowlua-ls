return {
    code = "test-event-decl",
    run = function(ctx)
        -- Report all events
        local all = ctx:find_event_declarations()
        for _, ev in ipairs(all) do
            local param_names = {}
            for _, p in ipairs(ev.params) do
                local desc = p.name .. ":" .. p.type_name
                if p.nilable then desc = desc .. "?" end
                if p.description then desc = desc .. "(" .. p.description .. ")" end
                param_names[#param_names + 1] = desc
            end
            local msg = ev.type_name .. "/" .. ev.event_name
            if #param_names > 0 then
                msg = msg .. " [" .. table.concat(param_names, ", ") .. "]"
            end
            if ev.source_uri then
                msg = msg .. " from=" .. ev.source_uri
            end
            local range = ev.range or {start = 0, ["end"] = 0}
            ctx:hint(range, msg)
        end

        -- Also report filtered results count
        local wow = ctx:find_event_declarations("WowEvent")
        if #wow > 0 then
            ctx:info({start = 0, ["end"] = 0}, "wow_count=" .. #wow)
        end
    end
}
