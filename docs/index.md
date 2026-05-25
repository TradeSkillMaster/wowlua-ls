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
  - title: WoW API out of the box
    details: "9,000+ API stubs for retail, classic, and classic era — loaded instantly, no setup needed. XML frame scanning types your templates and named frames automatically. Full .toc file editing with hover, completions, and diagnostics."
  - title: Event handlers, fully typed
    details: "SetScript handlers get typed automatically — self, event, and per-event payload params. Narrow event == \"ENCOUNTER_END\" and ... resolves to the exact payload types. Works with custom event systems too."
  - title: Metatable inference
    details: "Understands setmetatable + __index chains, __call, operator metamethods, and self-referential metatables. Your OOP patterns just work — no annotations needed."
  - title: Correlated narrowing
    details: "Check one return value, and the LS narrows the rest. Eliminates false positives from multi-return functions. Also infers correlated returns from your function bodies automatically."
  - title: 70 diagnostics
    details: "Type safety, nil checking, annotation correctness, code quality, and WoW-specific checks like wrong-flavor-api. Each one suppressible per-line or per-project. Write custom diagnostic plugins in Lua."
  - title: Cross-file intelligence
    details: "Addon namespace resolution, class inheritance across files, defclass factories, XML templates, and metatable chains — all resolved workspace-wide with parallel scanning. Multi-addon workspaces supported."
  - title: Flavor filtering
    details: "Declare target flavors (retail, classic, classic_era) and get warnings on APIs that don't exist in all your targets. WOW_PROJECT_ID guards and @flavor-narrows are understood."
  - title: CI-ready CLI
    details: "wowlua_ls check path/to/addon lints your addon from the command line. Exit code 1 on diagnostics — drop it straight into your CI pipeline."
  - title: LuaLS-compatible annotations
    details: "Uses the same ---@ annotation syntax you already know. Migrate incrementally — your existing annotations work, and wowlua-ls adds powerful WoW-specific extensions on top."
---
