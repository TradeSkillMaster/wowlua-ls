**New**
- Hover now shows a class's doc comment, plus the descriptions written after the type on `@field`, `@param` and `@return` lines ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations.html#documentation-comments))
- When you edit a doc comment, hovers in other files show the change right away, without waiting for a workspace rebuild

**Bug Fixes**
- `create-global` is now reported at a global's first write in source order
- JetBrains: the go-to-definition picker for symbols with several definitions now appears on every click, not only the first
