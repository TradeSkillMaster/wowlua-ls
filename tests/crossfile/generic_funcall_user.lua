---@diagnostic disable: unused-local
-- Cross-file generic funcall test: assigning a generic function call result
-- to an addon namespace field should NOT trigger field-type-mismatch.
-- Regression test: the cross-file scanner creates a placeholder table
-- annotation when the generic return type cannot be resolved, and assigning
-- a class-typed value to it was incorrectly flagged.

local addonName, ns = ...
ns.Instance = MakeInstance(ns.MixinA)
--    ^ hover: (field) Instance: table | MixinA
