package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.redhat.devtools.lsp4ij.LanguageServersRegistry
import com.redhat.devtools.lsp4ij.features.LSPPsiElement

/**
 * Stops IntelliJ from jumping to the top of the file when go-to-definition
 * (Ctrl/Cmd-click) lands on a token the language server can't resolve — a `{}`
 * table constructor, an operator, a literal, an undefined name, etc. Purely a
 * JetBrains/LSP4IJ workaround; VS Code is unaffected.
 *
 * Why the jump happens: our `.lua`/`.toc` files are mapped to the server by
 * file-name pattern rather than an IntelliJ language, so LSP4IJ backs them with a
 * *structureless* PSI file whose only PSI leaf is the whole file (offset 0). When
 * the server returns no definition, LSP4IJ's own `LSPGotoDeclarationHandler`
 * returns an empty result and IntelliJ falls back to `TargetElementUtil`, which —
 * via LSP4IJ's all-languages `LSPTargetElementEvaluator` — resolves the element at
 * the caret to that whole-file leaf and navigates to it, i.e. to the top of the
 * file. (Clicking a symbol *at its own definition* takes the same route: LSP4IJ
 * treats "target range contains the caret" as already-at-declaration and returns
 * empty.)
 *
 * IntelliJ takes the first `GotoDeclarationHandler` that returns a *non-empty*
 * result before it reaches that fallback. Registered `order="last"` (after
 * LSP4IJ's `order="first"` handler), this one runs only when the server found
 * nothing and returns a single zero-effect target at the caret, so navigation
 * stays put instead of jumping.
 *
 * It does not interfere with the two things it sits next to:
 * - Real go-to-definition: when the server resolves the token, LSP4IJ's
 *   order-first handler returns the real target and wins before this is consulted.
 * - Ctrl-*hover* (the underline/tooltip): LSP4IJ's order-first handler throws
 *   `ProcessCanceledException` (a `CancellationException`) while computing hover
 *   data, aborting the provider chain before this handler runs — so hovering a
 *   `{}` still highlights nothing.
 */
class WowLuaGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(
        sourceElement: PsiElement?,
        offset: Int,
        editor: Editor?,
    ): Array<PsiElement>? {
        val file = sourceElement?.containingFile ?: return null
        val ext = file.virtualFile?.extension?.lowercase()
        if (ext != "lua" && ext != "toc") return null
        // Only act on files LSP4IJ actually manages — mirrors the gate LSP4IJ's own
        // handler/evaluator use, so if the server is disabled we defer entirely.
        if (!LanguageServersRegistry.getInstance().isFileSupported(file)) return null

        // Reached only after LSP4IJ's order-first handler returned no definition.
        // Return a no-op target at the caret so IntelliJ navigates "here" (a
        // visual no-op) rather than falling back to the whole-file element.
        val length = file.textLength
        val start = offset.coerceIn(0, length)
        val end = (start + 1).coerceAtMost(length)
        return arrayOf(LSPPsiElement(file, TextRange(start, end)))
    }
}
