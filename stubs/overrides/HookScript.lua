---@meta _

---@class Frame
local Frame = {}

---@overload fun(self: Frame, script: "OnEvent", handler: fun(self: Frame, event: FrameEvent, ...params<FrameEvent>), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnUpdate", handler: fun(self: Frame, elapsed: number), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnShow", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnHide", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnMouseDown", handler: fun(self: Frame, button: string), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnMouseUp", handler: fun(self: Frame, button: string), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnEnter", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnLeave", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnLoad", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnClick", handler: fun(self: Frame, button: string, down: boolean), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnDragStart", handler: fun(self: Frame, button: string), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnDragStop", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnReceiveDrag", handler: fun(self: Frame), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnSizeChanged", handler: fun(self: Frame, width: number, height: number), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnKeyDown", handler: fun(self: Frame, key: string), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@overload fun(self: Frame, script: "OnKeyUp", handler: fun(self: Frame, key: string), bindingType?: LE_SCRIPT_BINDING_TYPE)
---@param scriptType ScriptFrame
---@param handler function
---@param bindingType? LE_SCRIPT_BINDING_TYPE
function Frame:HookScript(scriptType, handler, bindingType) end
