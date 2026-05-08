-- Cross-file test: file D uses select(2, ...).Field to extract a sub-table
local ns_sub = select(2, ...).DB
--     ^ hover: (local) ns_sub: {  def: local
--                            ^ hover: (field) DB: {
