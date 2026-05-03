-- Cross-file test: consuming addon namespace fields from expressions
local _, ns = ...

-- Concat fields should be string, not table
local sep = ns.PATTERNS.SEP
--    ^ hover: (local) sep: string  def: local
local pat = ns.PATTERNS.PAT_SEP
--    ^ hover: (local) pat: string  def: local
local inv = ns.PATTERNS.PAT_INV
--    ^ hover: (local) inv: string  def: local
local joined = ns.PATTERNS.JOINED
--     ^ hover: (local) joined: string  def: local

-- Arithmetic fields should be number
local base = ns.CONSTANTS.BASE
--    ^ hover: (local) base: number  def: local
local doubled = ns.CONSTANTS.DOUBLED
--    ^ hover: (local) doubled: number  def: local
local offset = ns.CONSTANTS.OFFSET
--     ^ hover: (local) offset: number  def: local

-- Unary expression fields
local neg = ns.CONSTANTS.NEG
--    ^ hover: (local) neg: number  def: local
local len = ns.CONSTANTS.LEN
--    ^ hover: (local) len: number  def: local

-- And-chain fields: should use the rightmost operand's type
local gs = ns.GUARDED.STR
--    ^ hover: (local) gs: string  def: local
local gn = ns.GUARDED.NUM
--    ^ hover: (local) gn: number  def: local
local gc = ns.GUARDED.CONCAT
--    ^ hover: (local) gc: string  def: local
local gd = ns.GUARDED.DEEP
--    ^ hover: (local) gd: string  def: local

-- Global variables with expression values
local cr = MY_CONCAT_RESULT
--    ^ hover: (local) cr: string  def: local
local ar = MY_ARITH_RESULT
--    ^ hover: (local) ar: number  def: local

-- Global variable with and-chain
local mg = MY_GUARDED
--    ^ hover: (local) mg: string  def: local
