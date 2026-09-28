**Bug Fixes**

- False `unused-function` on functions referenced only from the file that defines them.
- `unused-function` missed unused static functions that share a name with another table's function (e.g. `Settings.Reset` and `Cache.Reset`).
