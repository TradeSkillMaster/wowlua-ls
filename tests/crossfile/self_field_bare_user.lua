---@diagnostic disable: unused-local
-- Cross-file bare self-field test: consumer accessing inferred fields

---@class BareFieldChild : BareFieldClass
local Child = {}

function Child:Use()
    local d = self.db
    --              ^ hover: (field) db: BareDB  def: external
    local l = self.label
    --               ^ hover: (field) label: string  def: external
    local r = self.ready
    --               ^ hover: (field) ready: boolean  def: external
    local t = self.data
    --              ^ hover: (field) data: table  def: external
    local c = self.count
    --               ^ hover: (field) count: number  def: external
    local o = self.options
    --               ^ hover: (field) options: {\nname: string,\nscale: any\n}  def: external
    -- method call RHS falls back to any (pre-analysis scan can't resolve)
    local s = self.options.scale
    --                      ^ hover: (field) scale: any  def: external
    local n = self.options.name
    --                      ^ hover: (field) name: string  def: external
end
