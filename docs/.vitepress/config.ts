import { defineConfig } from 'vitepress'
import { readFileSync } from 'fs'
import { resolve } from 'path'

const loadGrammar = (file: string) =>
  JSON.parse(readFileSync(resolve(__dirname, '../../editors/vscode/syntaxes', file), 'utf-8'))

const luaGrammar = loadGrammar('lua.tmLanguage.json')
const tocGrammar = { ...loadGrammar('toc.tmLanguage.json'), name: 'toc' }

export default defineConfig({
  base: '/wowlua-ls/',
  title: 'wowlua-ls',
  description: 'A smarter language server for WoW addon development',

  head: [
    ['link', { rel: 'icon', type: 'image/png', href: '/wowlua-ls/logo.png' }],
  ],

  markdown: {
    languages: [luaGrammar, tocGrammar],
  },

  themeConfig: {
    logo: '/logo.png',

    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Reference', link: '/reference/annotations' },
      {
        text: 'Install',
        items: [
          { text: 'VS Code', link: 'https://marketplace.visualstudio.com/items?itemName=TradeSkillMaster.wowlua-ls' },
          { text: 'Open VSX', link: 'https://open-vsx.org/extension/TradeSkillMaster/wowlua-ls' },
          { text: 'JetBrains IDEs', link: 'https://plugins.jetbrains.com/plugin/31581-wow-lua-language-server' },
          { text: 'Zed, Neovim & others', link: '/guide/getting-started#installation' },
          { text: 'Releases', link: 'https://github.com/TradeSkillMaster/wowlua-ls/releases' },
        ],
      },
    ],

    sidebar: [
      {
        text: 'Introduction',
        items: [
          { text: 'Why wowlua-ls', link: '/guide/why-wowlua-ls' },
          { text: 'Getting Started', link: '/guide/getting-started' },
        ],
      },
      {
        text: 'Typing Your Code',
        items: [
          { text: 'Basic Annotations', link: '/guide/basic-annotations' },
          { text: 'Classes and Inheritance', link: '/guide/classes' },
          { text: 'Generics', link: '/guide/generics' },
          { text: 'Metatable Inference', link: '/guide/metatables' },
          { text: 'Multi-Return Functions', link: '/guide/multi-return' },
          { text: 'Event Payloads', link: '/guide/events' },
          { text: 'Expression Strings', link: '/guide/expressions' },
        ],
      },
      {
        text: 'Advanced Patterns',
        items: [
          { text: 'Nil Safety and Narrowing', link: '/guide/nil-safety' },
          { text: 'Builder Pattern', link: '/guide/builder-pattern' },
          { text: 'Custom Type Guards', link: '/guide/type-guards' },
          { text: 'Flavor Filtering', link: '/guide/flavor-filtering' },
          { text: 'Secret Values', link: '/guide/secrets' },
        ],
      },
      {
        text: 'Project Setup',
        items: [
          { text: 'Configuration', link: '/guide/configuration' },
          { text: 'Diagnostics', link: '/guide/diagnostics' },
          { text: 'TOC Files', link: '/guide/toc-files' },
          { text: 'XML Scanning', link: '/guide/xml-scanning' },
          { text: 'Diagnostic Plugins', link: '/guide/plugins' },
          { text: 'CLI Tools', link: '/guide/cli' },
        ],
      },
      {
        text: 'Quick Reference',
        items: [
          { text: 'All Annotations', link: '/reference/annotations' },
          { text: 'All Diagnostics', link: '/reference/diagnostics' },
          { text: 'Configuration Schema', link: '/reference/configuration' },
        ],
      },
      {
        text: 'Contributing',
        items: [
          { text: 'Development', link: '/guide/development' },
          { text: 'Stub Generation', link: '/guide/stub-generation' },
          { text: 'Adding a Diagnostic', link: '/guide/adding-diagnostics' },
          { text: 'Testing', link: '/guide/testing' },
        ],
      },
    ],

    socialLinks: [
      { icon: 'discord', link: 'https://discord.gg/XgqevqEqJK' },
      { icon: 'github', link: 'https://github.com/TradeSkillMaster/wowlua-ls' },
    ],

    search: {
      provider: 'local',
    },

  },
})
