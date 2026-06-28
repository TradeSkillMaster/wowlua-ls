-- Regression for the stubs/overrides/LibSharedMedia-3.0.lua library override:
-- `:Register` *adds* custom media types and the query methods must look them
-- back up, so `mediatype` is an arbitrary `string` (not the closed
-- `LibSharedMediaTypes` enum), and `:Register`'s `data` is `any` (custom types
-- store arbitrary values). The exhaustive diagnostic check verifies the custom
-- type registers and resolves with no false `type-mismatch`.
---@diagnostic disable: unused-local

local LSM = LibStub("LibSharedMedia-3.0")
LSM:Register("mycustomtype", "key", "data/path")
--  ^ hover: (method) function LibSharedMedia-3.0:Register(
LSM:Register("font", "OtherFont", "data/path2")
LSM:Register("nineslice", "key", { file = "x", width = 1 }) -- arbitrary table data

local fetched = LSM:Fetch("mycustomtype", "key") -- query a custom (registered) type
local mediaList = LSM:List("mycustomtype")
local isValid = LSM:IsValid("mycustomtype", "key")
