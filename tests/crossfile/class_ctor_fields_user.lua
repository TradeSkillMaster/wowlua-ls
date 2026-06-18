local _, ns = ... ---@class ClassCtorFieldsNS

local StatusType = ns.StatusType
local ItemKind = ns.ItemKind

local _a = StatusType.Active
--                    ^ hover: (field) Active: string
local _b = StatusType.Pending
--                    ^ hover: (field) Pending: number
local _c = StatusType.Enabled
--                    ^ hover: (field) Enabled: boolean
-- Constructor fields should not produce undefined-field
local _d = StatusType.Inactive
--                    ^ hover: (field) Inactive: string

-- @field annotations should still work alongside constructor fields
local _e = ItemKind.Weapon
--                  ^ hover: (field) Weapon: string
local _f = ItemKind.Special
--                  ^ hover: (field) function ItemKind.Special()

-- Global assignment class constructor fields should also work
---@type GlobalClassCtor
local _gobj = {} ---@type GlobalClassCtor
local _g = _gobj.Foo
--               ^ hover: (field) Foo: string
local _h = _gobj.Bar
--               ^ hover: (field) Bar: number

-- Function-call constructor fields should not produce undefined-field cross-file
local CallCtorFields = ns.CallCtorFields
local _call1 = CallCtorFields.FromCall
--                            ^ hover: (field) FromCall: any
local _call2 = CallCtorFields.FromMethod
--                            ^ hover: (field) FromMethod: any
local _call3 = CallCtorFields.Literal
--                            ^ hover: (field) Literal: string

-- Expression-based constructor fields should resolve cross-file
local ExprFields = ns.ExprFields
local _cmp = ExprFields.CompareResult
--                      ^ hover: (field) CompareResult: boolean
local _and = ExprFields.LogicalAnd
--                      ^ hover: (field) LogicalAnd: boolean
local _chain = ExprFields.ChainedExpr
--                        ^ hover: (field) ChainedExpr: boolean
local _neg = ExprFields.Negated
--                      ^ hover: (field) Negated: boolean
local _cat = ExprFields.Concat
--                      ^ hover: (field) Concat: string
local _arith = ExprFields.Arithmetic
--                        ^ hover: (field) Arithmetic: number
local _len = ExprFields.HashLen
--                      ^ hover: (field) HashLen: number
local _or = ExprFields.OrFallback
--                     ^ hover: (field) OrFallback: string
local _negexpr = ExprFields.NegExpr
--                          ^ hover: (field) NegExpr: number
local _lit = ExprFields.Literal
--                      ^ hover: (field) Literal: string

-- Constructor fields from a @class declared inside a function body must resolve
-- cross-file (regression: a value typed as the nested class previously reported
-- undefined-field on these constructor-inferred fields).
---@type NestedCtorFields
local nested = {}
local _nf1 = nested.skyridingEnabled
--                  ^ hover: (field) skyridingEnabled: boolean
local _nf2 = nested.reportPurchases
--                  ^ hover: (field) reportPurchases: boolean
local _nf3 = nested.rideAlong
--                  ^ hover: (field) rideAlong: number
