---
layout: home

hero:
  name: wowlua-ls
  text: A smarter language server for WoW addons
  tagline: Deep type inference, nil safety, and first-class WoW API support. Built for addon developers who want their tools to actually understand their code.
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: Why wowlua-ls?
      link: /guide/why-wowlua-ls

features:
  - title: Real type inference
    details: Understands setmetatable, __index chains, builder patterns, and correlated returns without you annotating everything. Your OOP patterns just work.
  - title: Nil safety that isn't annoying
    details: Tracks nil through multi-return values, if/else branches, early exits, and assert() calls. Narrowing is automatic and correlated — guard one value, siblings follow.
  - title: 55+ diagnostics
    details: From basic type mismatches to flavor-specific API availability, undeclared field injection, and unused locals. Each one suppressible per-line or per-project.
  - title: WoW API out of the box
    details: "Ships with complete retail + classic API stubs. SetScript handlers get typed automatically — self, event, and per-event payload params via ... narrowing. Full editing support for .toc files with hover, completions, and diagnostics. Configure target flavors to catch API mismatches."
  - title: Cross-file intelligence
    details: Addon namespace resolution, class inheritance across files, defclass factories, and metatable chains — all resolved workspace-wide with parallel scanning.
  - title: LuaLS-compatible annotations
    details: Uses the same annotation syntax you already know. Migrate incrementally — your existing annotations work, and wowlua-ls adds powerful features on top.
---
