package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * IntelliJ navigates with the first go-to-declaration handler that returns
 * targets. LSP4IJ's handler returns only the first definition once a word has a
 * reference semantic token, so ours must be consulted before it for a symbol with
 * several definitions to keep showing the picker.
 */
class MultiDefinitionGotoHandlerOrderTest : BasePlatformTestCase() {
    fun testRunsBeforeLsp4ijHandler() {
        val handlers = GotoDeclarationHandler.EP_NAME.extensionList
        val ours = handlers.indexOfFirst { it is WowLuaMultiDefinitionGotoHandler }
        val lsp4ij = handlers.indexOfFirst {
            it.javaClass.name == "com.redhat.devtools.lsp4ij.features.navigation.LSPGotoDeclarationHandler"
        }
        assertTrue("our handler is registered", ours >= 0)
        assertTrue("LSP4IJ's handler is registered", lsp4ij >= 0)
        assertTrue("ours ($ours) runs before LSP4IJ's ($lsp4ij)", ours < lsp4ij)
    }
}
