### Bug Fixes

- Fix class type resolution failing when the variable name differs from the class name
- Fix addon namespace `@class` fields leaking across addons in multi-addon workspaces
- Fix IntelliJ LSP freeze after extended use
- Fix cross-file function return type resolution failing to propagate types across files
- Fix go-to-definition pointing to the wrong file for class fields defined in cross-file classes
- Fix `undefined-doc-name` diagnostic highlighting the entire function body instead of just the annotation

### Improvements

- Propagate generic class type parameters into inline callback parameters — e.g. when a `Pool<T>` method takes `fun(item: T)`, the callback parameter now correctly resolves `T` from the receiver's type args ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/generics.html))
- Add second return value (`isNew`) to `ObjectPool.Acquire` and `FramePool.Acquire` stubs
