local _, ns = ... ---@class ClassCtorFieldsNS

local StatusType = ns.StatusType
local ItemKind = ns.ItemKind

local _a = StatusType.Active
--                    ^ hover: (field) Active: string  diag: none
local _b = StatusType.Pending
--                    ^ hover: (field) Pending: number  diag: none
local _c = StatusType.Enabled
--                    ^ hover: (field) Enabled: boolean  diag: none
-- Constructor fields should not produce undefined-field
local _d = StatusType.Inactive
--                    ^ hover: (field) Inactive: string  diag: none

-- @field annotations should still work alongside constructor fields
local _e = ItemKind.Weapon
--                  ^ hover: (field) Weapon: string  diag: none
local _f = ItemKind.Special
--                  ^ hover: (field) Special: function  diag: none

-- Global assignment class constructor fields should also work
---@type GlobalClassCtor
local _gobj = {} ---@type GlobalClassCtor
local _g = _gobj.Foo
--               ^ hover: (field) Foo: string  diag: none
local _h = _gobj.Bar
--               ^ hover: (field) Bar: number  diag: none

-- Expression-based constructor fields should resolve cross-file
local ExprFields = ns.ExprFields
local _cmp = ExprFields.CompareResult
--                      ^ hover: (field) CompareResult: boolean  diag: none
local _and = ExprFields.LogicalAnd
--                      ^ hover: (field) LogicalAnd: boolean  diag: none
local _chain = ExprFields.ChainedExpr
--                        ^ hover: (field) ChainedExpr: boolean  diag: none
local _neg = ExprFields.Negated
--                      ^ hover: (field) Negated: boolean  diag: none
local _cat = ExprFields.Concat
--                      ^ hover: (field) Concat: string  diag: none
local _arith = ExprFields.Arithmetic
--                        ^ hover: (field) Arithmetic: number  diag: none
local _len = ExprFields.HashLen
--                      ^ hover: (field) HashLen: number  diag: none
local _or = ExprFields.OrFallback
--                     ^ hover: (field) OrFallback: string  diag: none
local _negexpr = ExprFields.NegExpr
--                          ^ hover: (field) NegExpr: number  diag: none
local _lit = ExprFields.Literal
--                      ^ hover: (field) Literal: string  diag: none
