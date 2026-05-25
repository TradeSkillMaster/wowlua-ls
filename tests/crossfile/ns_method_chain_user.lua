---@diagnostic disable: undefined-global
-- Cross-file test: exercises method chains on @class-typed addon namespace fields.
-- Regression test: methods defined on ns.Foo must be available on class Foo when
-- resolved via generic return types (e.g. From("Foo"):Include("Bar")).

-- Direct access via select(2, ...) — resolves to the class type
local NsMcComponent = select(2, ...).NsMcComponent
--     ^ hover: (local) NsMcComponent: NsMcComponent {
--                                   ^ hover: (field) NsMcComponent: NsMcComponent {

-- Direct Include resolves through the sub-table
local Svc = NsMcComponent:Include("NsMcService")
--    ^ hover: (local) Svc: NsMcService {

-- From() returns the class, which must also have Include method
local Svc2 = NsMcComponent:From("NsMcComponent"):Include("NsMcService")
--    ^ hover: (local) Svc2: NsMcService {

-- Method on the resolved service should work
Svc:GetCount()
--  ^ diag: none
