---@meta _

-- Types defined in a @meta file should be visible across files
-- without producing undefined-doc-name warnings.

---@alias MetaFrameTable table<string, MetaFrameData>

---@class MetaFrameData
---@field x number
---@field y number
