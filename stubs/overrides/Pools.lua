---@meta _
-- Pool types and factory functions with generic type parameters.
-- ObjectPool/FramePool/FramePoolCollection are FrameXML-defined (not in Blizzard's
-- APIDocumentationGenerated, Ketho's vscode-wow-api, or BlizzardInterfaceResources),
-- so they have no upstream source to fix in stub_gen.rs and live here as overrides.

-- ObjectPool<T>: a pool whose Acquire() returns T.
---@class ObjectPool<T>
local ObjectPool = {}

---Acquires an object from the pool, creating one if necessary.
---@return T
---@return boolean isNew
function ObjectPool:Acquire() end

---Releases the given object back into the pool.
---@param obj T
function ObjectPool:Release(obj) end

---Releases all active objects back into the pool.
function ObjectPool:ReleaseAll() end

---Returns the number of currently active objects in the pool.
---@return number
function ObjectPool:GetNumActive() end

---Returns an iterator over all currently active objects in the pool.
---@return fun(): T
function ObjectPool:EnumerateActive() end

---Returns an iterator over all currently inactive objects in the pool.
---@return fun(): T
function ObjectPool:EnumerateInactive() end

-- FramePool<T, Tp>: a pool whose Acquire() returns T & Tp (frame + template mixin).
-- Addon code commonly writes FramePool<Frame, SomeTemplate> (2 params) or
-- FramePool<Frame> (1 param). When Tp is omitted it resolves to any, and
-- T & any simplifies to T, which is still a useful type.
--
-- NOTE: FramePool inherits from ObjectPool<T & Tp> but all methods are re-declared
-- explicitly because the LS does not substitute generic type parameters through
-- inheritance chains (e.g. T→T&Tp from the parent), so omitting them would leave
-- Acquire/Release/Enumerate returning '?' for callers typed as FramePool<T,Tp>.
---@class FramePool<T, Tp>: ObjectPool<T & Tp>
local FramePool = {}

---Acquires a frame from the pool, creating one if necessary.
---@return T & Tp
---@return boolean isNew
function FramePool:Acquire() end

---Releases the given frame back into the pool.
---@param obj T & Tp
function FramePool:Release(obj) end

---Releases all active frames back into the pool.
function FramePool:ReleaseAll() end

---Returns the number of currently active frames in the pool.
---@return number
function FramePool:GetNumActive() end

---Returns an iterator over all currently active frames in the pool.
---@return fun(): T & Tp
function FramePool:EnumerateActive() end

---Returns an iterator over all currently inactive frames in the pool.
---@return fun(): T & Tp
function FramePool:EnumerateInactive() end

-- FramePoolCollection: returned by CreateFramePoolCollection.
-- GetOrCreatePool returns a typed FramePool<T, Tp> matching CreateFrame semantics.
---@class FramePoolCollection
local FramePoolCollection = {}

---Creates a new pool for the given frame type and template, adding it to the collection.
---@generic T, Tp
---@param frameType `T` | FrameType
---@param parent? any
---@param template? `Tp` | string
---@param resetFunc? function
---@param forbidden? boolean
---@return FramePool<T, Tp>
function FramePoolCollection:CreatePool(frameType, parent, template, resetFunc, forbidden) end

---Returns the pool for the given frame arguments, creating it if necessary.
---@generic T, Tp
---@param frameType `T` | FrameType
---@param parent? any
---@param template? `Tp` | string
---@param resetFunc? function
---@param forbidden? boolean
---@return FramePool<T, Tp>
function FramePoolCollection:GetOrCreatePool(frameType, parent, template, resetFunc, forbidden) end

---Acquires a frame from the appropriate pool for the given frame type.
---When both frameType and template are string literals, returns the intersection
---type matching what GetOrCreatePool/CreatePool would have produced.
---@generic T, Tp
---@overload fun(self: FramePoolCollection, frameType: `T`|FrameType, parent?: any, template: `Tp`|string): T & Tp, boolean
---@param frameType FrameType
---@param parent? any
---@param template? string
---@return Frame
---@return boolean isNew
function FramePoolCollection:Acquire(frameType, parent, template) end

---Releases all active objects in all pools back into their respective pools.
function FramePoolCollection:ReleaseAll() end

---Returns an iterator over all currently active objects across all pools.
---@return fun(): Frame
function FramePoolCollection:EnumerateActive() end

---Creates a new pool of objects using a custom creation function.
---@generic T
---@param createFunc fun(): T
---@param resetFunc? fun(pool: ObjectPool<T>, obj: T)
---@param capacity? number
---@return ObjectPool<T>
function CreateObjectPool(createFunc, resetFunc, capacity) end

---Creates a new pool of objects using a custom creation function (unsecured).
---@generic T
---@param createFunc fun(): T
---@param resetFunc? fun(pool: ObjectPool<T>, obj: T)
---@param capacity? number
---@return ObjectPool<T>
function CreateUnsecuredObjectPool(createFunc, resetFunc, capacity) end

---Creates a new pool of frames. Like CreateFrame, returns FramePool<T, Tp> when
---a template is provided so that Acquire() returns the frame+template intersection.
---@generic T, Tp
---@overload fun(frameType: `T`|FrameType, parent?: any, template: `Tp`|string, resetFunc?: function, forbidden?: boolean, postCreate?: function, capacity?: number): FramePool<T, Tp>
---@overload fun(frameType: `T`|FrameType, parent?: any, resetFunc?: function, forbidden?: boolean, postCreate?: function, capacity?: number): ObjectPool<T>
---@param frameType `T` | FrameType
---@param parent? any
---@param template? `Tp` | string
---@param resetFunc? function
---@param forbidden? boolean
---@param postCreate? function
---@param capacity? number
---@return ObjectPool<T>
function CreateFramePool(frameType, parent, template, resetFunc, forbidden, postCreate, capacity) end

---Creates a new secure pool of frames.
---@generic T, Tp
---@overload fun(frameType: `T`|FrameType, parent?: any, template: `Tp`|string, resetFunc?: function, forbidden?: boolean, postCreate?: function, capacity?: number): FramePool<T, Tp>
---@overload fun(frameType: `T`|FrameType, parent?: any, resetFunc?: function, forbidden?: boolean, postCreate?: function, capacity?: number): ObjectPool<T>
---@param frameType `T` | FrameType
---@param parent? any
---@param template? `Tp` | string
---@param resetFunc? function
---@param forbidden? boolean
---@param postCreate? function
---@param capacity? number
---@return ObjectPool<T>
function CreateSecureFramePool(frameType, parent, template, resetFunc, forbidden, postCreate, capacity) end

---Creates a new secure pool of objects.
---@generic T
---@param createFunc fun(): T
---@param resetFunc? fun(pool: ObjectPool<T>, obj: T)
---@param capacity? number
---@return ObjectPool<T>
function CreateSecureObjectPool(createFunc, resetFunc, capacity) end

---Creates a new collection of frame pools (one pool per frame type/template combination).
---@return FramePoolCollection
function CreateFramePoolCollection() end

---Creates a new secure collection of frame pools.
---@return FramePoolCollection
function CreateSecureFramePoolCollection() end

---Creates a new collection of font string pools.
---@return FramePoolCollection
function CreateFontStringPoolCollection() end

---Creates a new pool of font strings.
---@param parent Frame?
---@param layer? string
---@param template? string
---@param resetFunc? function
---@return ObjectPool<FontString>
function CreateFontStringPool(parent, layer, template, resetFunc) end

---Creates a new pool of textures.
---@param parent Frame?
---@param layer? string
---@param subLayer? string
---@param template? string
---@param resetFunc? function
---@return ObjectPool<Texture>
function CreateTexturePool(parent, layer, subLayer, template, resetFunc) end

---Creates a new pool of mask textures.
---@param parent Frame?
---@param layer? string
---@param subLayer? string
---@param template? string
---@param resetFunc? function
---@return ObjectPool<MaskTexture>
function CreateMaskTexturePool(parent, layer, subLayer, template, resetFunc) end

---Creates a new pool of actors.
---@param parent Frame?
---@param template? string
---@param resetFunc? function
---@return ObjectPool<Actor>
function CreateActorPool(parent, template, resetFunc) end
