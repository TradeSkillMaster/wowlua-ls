-- Test plugin: exercises find_locals, field_reads, field_writes, and init:fields().
-- Warns when a table-initialized local has fields that are read but never declared,
-- or declared but never read or written.
return {
  code = "test-field-tracker",
  ---@param ctx wowlua.plugin.FileContext
  run = function(ctx)
    for _, var in ipairs(ctx:find_locals({init = "table"})) do
      local init = var.init
      if init then
        local declared = {}
        for _, f in ipairs(init:fields()) do
          declared[f.name] = {
            range = f.range,
            value_kind = f.value_kind,
            read = false,
            written = false,
          }
        end

        for _, access in ipairs(var:field_reads()) do
          if not declared[access.field_name] then
            ctx:warn(access.range, "Reading undeclared field: " .. access.field_name)
          else
            declared[access.field_name].read = true
          end
        end

        for _, access in ipairs(var:field_writes()) do
          local info = declared[access.field_name]
          if info then
            info.written = true
          end
        end

        for name, info in pairs(declared) do
          if not info.read and not info.written then
            ctx:hint(info.range, "Declared field never used: " .. name)
          end
        end
      end
    end
  end
}
