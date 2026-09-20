package com.tradeskillmaster.wowluals

import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * A platform canary: double-clicking a word inside a fenced `lua` block of a README
 * selects that word.
 *
 * Through IntelliJ 2026.1 the fence is not injected (no Lua language exists), so its
 * body stays plain markdown PSI whose smallest element is a whole line, and only
 * IntelliJ's word selectioner keeps a double-click from swallowing that line. This
 * fixture can only catch the platform changing that: `BasePlatformTestCase` starts no
 * language server, and `LSPWordSelectionFilter` returns early when there are none, so
 * the word selectioner is always enabled here regardless of our file mappings or
 * client features. It cannot fail because of a mapping or feature regression on our
 * side — [SelectionRangeFeatureTest] is what covers those.
 *
 * From 2026.2 the fence is injected as a TextMate fragment named `<host>.lua`, which
 * our mapping matches by name; see that test for the guard that keeps word selection
 * working there.
 */
class MarkdownCodeFenceSelectionTest : BasePlatformTestCase() {
    private fun assertDoubleClickSelects(expected: String, text: String) {
        myFixture.configureByText("README.md", text)
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_SELECT_WORD_AT_CARET)
        assertEquals(expected, myFixture.editor.selectionModel.selectedText)
    }

    fun testWordInLuaFence() {
        assertDoubleClickSelects(
            "GetNodeInfo",
            """
            # Title

            ```lua
            local info = LibTalentTree:GetNode<caret>Info(nodeID)
            ```
            """.trimIndent(),
        )
    }

    fun testWordInMultiLineLuaFence() {
        assertDoubleClickSelects(
            "entryInfo",
            """
            ```lua
            local nodeInfo = LibTalentTree:GetNodeInfo(nodeID)
            local entry<caret>Info = LibTalentTree:GetEntryInfo(entryID)
            print(nodeInfo, entryInfo)
            ```
            """.trimIndent(),
        )
    }

    fun testWordInProse() {
        assertDoubleClickSelects("library", "This lib<caret>rary provides talent tree info.")
    }
}
