---@diagnostic disable: unused-local
-- Cross-file test: accessing namespace fields that were assigned from locals
local addonName, addon = ...

-- Should resolve to Texture (from CreateTexture return type)
local sa = addon.SurgeArc
--    ^ hover: (local) sa: Texture {  def: local

-- Should resolve to Frame (from CreateFrame with "Frame" arg)
local td = addon.TextDisplay
--    ^ hover: (local) td: Frame {  def: local

-- Should resolve to Texture (from CreateTexture return type)
local tb = addon.TextBackground
--    ^ hover: (local) tb: Texture {  def: local

-- A local's non-Simple @type annotation (union) must survive cross-file
-- (regression: assigning a typed local to a namespace field collapsed it to `any`).
local ion = addon.L2N_IdOrName
--    ^ hover: (local) ion: number | string

-- A `table<K,V>` @type must survive too (regression: degraded to a bare `table`).
local lbl = addon.L2N_Labels
--    ^ hover: (local) lbl: table<string, string>

-- A plain scalar-literal local infers its type cross-file (regression: was `any`).
local rc = addon.L2N_RetryCount
--    ^ hover: (local) rc: number

-- Precedence: an explicit compound @type on the source local overrides the
-- call's inferred @return (regression: the inferred `Frame` won over `@type`).
local w = addon.L2N_TypedWidget
--    ^ hover: (local) w: Frame | Texture

-- Control: no @type on the source local, so the inferred @return (Frame) is used.
local pw = addon.L2N_InferredWidget
--    ^ hover: (local) pw: Frame {
