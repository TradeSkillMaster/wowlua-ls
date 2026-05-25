---@meta _
-- Override pool types and factory functions to provide generic type parameters.
-- The vendor stubs define ObjectPoolBaseMixin/PoolCollectionBaseMixin as plain
-- (non-generic) mixin classes. The public factory functions return proxy objects
-- whose Acquire() method returns a typed object — expressed here via generics.

-- ObjectPool<T>: a pool whose Acquire() returns T.
-- Inherits non-generic utility methods (ReleaseAll, GetNumActive, etc.) from
-- ObjectPoolBaseMixin which are registered via the semicolon-fix in
-- extract_inline_class_with_offset (annotation_scanning.rs).
---@class ObjectPool<T>: ObjectPoolBaseMixin
local ObjectPool = {}

---Acquires an object from the pool, creating one if necessary.
---@return T
function ObjectPool:Acquire() end

---Releases the given object back into the pool.
---@param obj T
function ObjectPool:Release(obj) end

---Returns an iterator over all currently active objects in the pool.
---@return fun(): T
function ObjectPool:EnumerateActive() end

-- FramePool<T, Tp>: a pool whose Acquire() returns T & Tp (frame + template mixin).
-- Addon code commonly writes FramePool<Frame, SomeTemplate> (2 params) or
-- FramePool<Frame> (1 param). When Tp is omitted it resolves to any, and
-- T & any simplifies to T, which is still a useful type.
---@class FramePool<T, Tp>: ObjectPoolBaseMixin
local FramePool = {}

---Acquires a frame from the pool, creating one if necessary.
---@return T & Tp
function FramePool:Acquire() end

---Releases the given frame back into the pool.
---@param obj T & Tp
function FramePool:Release(obj) end

---Returns an iterator over all currently active frames in the pool.
---@return fun(): T & Tp
function FramePool:EnumerateActive() end

-- FramePoolCollection: returned by CreateFramePoolCollection.
-- GetOrCreatePool returns a typed FramePool<T, Tp> matching CreateFrame semantics.
---@class FramePoolCollection: PoolCollectionBaseMixin
local FramePoolCollection = {}

---Returns the pool for the given frame arguments, creating it if necessary.
---@generic T, Tp
---@param frameType `T` | FrameType
---@param parent? any
---@param template? `Tp` | string
---@param resetFunc? function
---@param forbidden? boolean
---@return FramePool<T, Tp>
function FramePoolCollection:GetOrCreatePool(frameType, parent, template, resetFunc, forbidden) end

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
