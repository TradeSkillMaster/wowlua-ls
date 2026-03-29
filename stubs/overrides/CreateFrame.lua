---@meta _
-- Override CreateFrame to return intersection T & Tp when a template is provided.
-- The upstream stub uses @return table|T|Tp (a union), but the actual semantics
-- are that the returned frame has ALL properties of both the frame type and the
-- template mixin — an intersection.
-- Without a template, the overload returns just T.

---@generic T, Tp
---@overload fun(frameType: `T`|FrameType, name?: string, parent?: any, template: `Tp`|Template, id?: number): T & Tp
---@overload fun(frameType: `T`|FrameType, name?: string, parent?: any): T
---@param frameType `T` | FrameType
---@param name? string
---@param parent? any
---@param template? `Tp` | Template
---@param id? number
---@return T frame
function CreateFrame(frameType, name, parent, template, id) end
