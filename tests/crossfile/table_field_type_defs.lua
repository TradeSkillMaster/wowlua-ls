-- Cross-file `@class` field-type harvesting: the bare-`table` placeholder slice.
--
-- A top-level `Class.field = <call>` whose callee the coarse cross-file scan can't
-- follow parks a bare `Table(None)` placeholder (build_on_stubs' non-namespace-root
-- FunctionCall site), NOT `any`. Unlike `any`, this placeholder carries no author
-- annotation either, so `had_annotation_at_build` is false and sharpening it can't
-- regress `field-type-mismatch`. On first cross-file read, `ensure_field_overlay`
-- harvests the definition-site type — the `table`->precise slice of the
-- lossless-cross-file work. See analysis/deferred.rs (`field_is_coarse_placeholder`).

local addonName, ns = ...

---@class TFT_Widget
local Widget = {}
ns.TFT_Widget = Widget
function Widget:Ping() end

---@class TFT_Factory
local Factory = {}
---@return TFT_Widget
function Factory:Make() end
---@return number
function Factory:Count() end

---@type TFT_Factory
local f

---@class TFT_Reg
local Reg = {}
ns.TFT_Reg = Reg

-- The coarse cross-file scan can't follow the local `f`'s method call, so it parks a
-- bare `Table(None)` placeholder on `built`; the full engine resolves it to TFT_Widget.
Reg.built = f:Make()

-- A `Table(None)` placeholder can even upgrade to a non-table primitive (the
-- `select(3, ...)`-style case build_on_stubs documents), proving the harvest reads the
-- real RHS type rather than the coarse `table`.
Reg.num = f:Count()
