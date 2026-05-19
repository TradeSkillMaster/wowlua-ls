-- Cross-file self-field test: accessing fields via @param-typed function parameter

--- @param builder ParamFieldClass
local function useBuilder(builder)
    local d = builder.db
    --                 ^ hover: (field) db: table  def: external
    local l = builder.label
    --                  ^ hover: (field) label: string  def: external
    local o = builder.opts
    --                  ^ hover: (field) opts: table  def: external
    builder:DoWork()
    --       ^ hover: (method) function ParamFieldClass:DoWork()
end
