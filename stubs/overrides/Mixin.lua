---@meta _
-- Override Mixin, CreateFromMixins, and CreateAndInitFromMixin to return
-- intersection types that combine the object/mixin type parameters.
-- Uses variadic generics (`...M`) so any number of mixins is supported.

-- Mixin(object, ...) copies fields from N mixin tables onto `object` and
-- returns it.  The return type is object & mixin1 & mixin2 & ...
-- @narrows-arg 1 means bare calls (`Mixin(f, M)` without capturing the
-- return) narrow the first argument's type in-place.
---@generic T, ...M
---@narrows-arg 1
---@param object T
---@param ... any
---@return T & ...M
function Mixin(object, ...) end

-- CreateFromMixins(...) creates a new empty table and copies from N mixins.
-- The return type is mixin1 & mixin2 & ...
---@generic ...M
---@param ... any
---@return ...M
function CreateFromMixins(...) end

-- CreateAndInitFromMixin(mixin, ...) creates from a single mixin, calls
-- mixin:Init(...) on it, and returns the new object.
---@generic T
---@param mixin T
---@param ... any
---@return T
function CreateAndInitFromMixin(mixin, ...) end
