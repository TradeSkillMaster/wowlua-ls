**Bug Fixes**
- Fixed go-to-definition, hover, and find-references landing on the wrong method when two same-typed local variables each define a method of the same name — both within a single file and across files.
- Regenerated the bundled WoW API stubs to pick up the many new symbols added upstream since the last regeneration, and updated stub generation's wiki fetch to handle warcraft.wiki.gg's new `API:` namespace structure (adds ~6,700 symbols and ~2,950 functions).
