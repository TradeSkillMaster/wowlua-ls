**Bug Fixes**
- `unused-function` now updates as you edit. Adding or removing a call in another file updates the hint right away, instead of only after the next full workspace rebuild.
- JetBrains: comment delimiters (`--`, `--[[`, `]]`) now show in the comment color instead of the punctuation color in color schemes that give punctuation its own color, such as the New UI's Dark theme.

**New**
- Hovers now link the classes and aliases they name to where those types are declared. In JetBrains IDEs, clicking a type name opens that type's hover in the same popup, and <kbd>F4</kbd> jumps to its declaration. Other editors show a **Go to:** line at the bottom of the hover. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/basic-annotations.html#documentation-comments))

**Improvements**
- A class's hover now lists its fields in the order they're defined: its `@field`s as written, then the fields and methods its code adds, then the ones it inherits.
- Double-click and expand-selection no longer wait for the file's analysis to finish.
