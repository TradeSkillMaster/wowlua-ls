---@meta _
-- Override UnitFrame type to use the actual template class (NamePlateUnitFrameTemplate)
-- instead of the base element type (Button). The template class already inherits from
-- Button and includes additional child fields (castBar, WidgetContainer, etc.).

---@class NamePlateBaseMixin
---@field UnitFrame NamePlateUnitFrameTemplate
