package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.psi.PsiManager
import com.intellij.testFramework.LightVirtualFile
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The selection-range feature must refuse files LSP4IJ itself would not hand to
 * the server.
 *
 * IntelliJ 2026.2 injects a ```lua markdown fence as a TextMate fragment named
 * `<host>.lua` — an in-memory file matching our `*.lua` mapping. LSP4IJ then
 * disables IntelliJ's word selectioner for it (that check has no in-memory-file
 * guard) while its own selection handler skips it (that one does), leaving a
 * double-click in a README fence to select the whole code block. Re-applying the
 * guard here keeps IntelliJ's native word selection in charge.
 */
class SelectionRangeFeatureTest : BasePlatformTestCase() {
    fun testInjectedFenceFragmentIsNotClaimed() {
        val feature = WowLuaLanguageServerFactory().createClientFeatures().selectionRangeFeature
        // The shape IntelliJ gives an injected markdown code fence.
        val fragment = LightVirtualFile("README.md.lua", "local frame = CreateFrame(\"Frame\")\n")
        val psiFile = PsiManager.getInstance(project).findFile(fragment)!!
        // Also proves the guard runs first: the capability check behind it needs a
        // live server wrapper this fixture has no way to provide.
        assertFalse(feature.isSelectionRangeSupported(psiFile))
    }
}
