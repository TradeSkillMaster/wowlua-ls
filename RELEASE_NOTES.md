**Bug Fixes**
- Fixed runtime field additions (e.g. `self.field = ...`) not propagating to other open files until the edited file was reopened, in cases where completion or signature help fired mid-edit.
