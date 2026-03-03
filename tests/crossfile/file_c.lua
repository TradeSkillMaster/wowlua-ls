-- Cross-file test: file C uses select(2, ...) to extract addon namespace
local ns = select(2, ...)
local v = ns.version
--    ^ hover: v: number  def: local
local t = ns.title
--    ^ hover: t: string  def: local
ns.DB:Start()
--        ^ hover: Start: fun()  def: external
