### Bug Fixes

- Fixed completion breaking after a commented-out line in builder chains.
- Restored shadowed FrameXML globals to the stubs, so those globals once again resolve correctly.

### Improvements

- Added missing `FrameEvents` discovered from `wow-ui-source` `RegisterEvent` calls, improving event name completion and hover coverage.
