package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.progress.ProcessCanceledException
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.util.ProgressIndicatorUtils
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.redhat.devtools.lsp4ij.LSPIJUtils
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.LanguageServersRegistry
import com.redhat.devtools.lsp4ij.client.features.FileUriSupport
import com.redhat.devtools.lsp4ij.features.LSPPsiElementFactory
import org.eclipse.lsp4j.DefinitionParams
import org.eclipse.lsp4j.TextDocumentIdentifier

/**
 * Keeps go-to-definition offering a picker every time a symbol has more than one
 * definition (a global or method defined in several files), instead of only on
 * the first Ctrl/middle-click.
 *
 * Why LSP4IJ alone shows it once: our `.lua` files get LSP4IJ's semantic-token
 * view provider, and its `LSPGotoDeclarationHandler` answers a token that the
 * provider classifies as a *reference* through `LSPSemanticTokenPsiReference
 * .resolve()`, which keeps only the first definition. A token becomes a reference
 * either because the server colors it (a function call name) or because LSP4IJ's
 * handler itself tags the word as a `type` token the first time it finds targets
 * there (on that click, or merely Ctrl-hovering it). So a picker shows at most once
 * per word, and every later click jumps straight to the first definition.
 *
 * Registered before LSP4IJ's handler, this one asks the server directly (public
 * LSP4IJ API only) and answers when there are two or more targets. Otherwise —
 * one or no target, the caret on one of the definitions, a failed request — it
 * returns null and LSP4IJ's handler runs as before (single-target jumps,
 * "already at declaration" → usages, the Ctrl-hover underline).
 */
class WowLuaMultiDefinitionGotoHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(
        sourceElement: PsiElement?,
        offset: Int,
        editor: Editor?,
    ): Array<PsiElement>? {
        val file = sourceElement?.containingFile ?: return null
        val virtualFile = file.virtualFile ?: return null
        if (virtualFile.extension?.lowercase() != "lua") return null
        if (!LanguageServersRegistry.getInstance().isFileSupported(file)) return null
        // Ctrl-hover computes the underline through the same handlers; leave that
        // to LSP4IJ, whose reference path gives the word-sized underline. The click
        // itself runs from the go-to-declaration action, not from this handler.
        if (isCtrlHover()) return null
        val document = editor?.document ?: return null

        val targets = try {
            requestTargets(file.project, virtualFile, document, offset)
        } catch (e: ProcessCanceledException) {
            // Rethrow a user cancellation; a request the server cancelled falls through.
            ProgressManager.checkCanceled()
            null
        } catch (e: Exception) {
            // Server error, crash or restart: let LSP4IJ's handler take the click.
            null
        } ?: return null
        if (targets.size < 2) return null
        // At one of its own definitions, leave the click to LSP4IJ, which treats a
        // declaration token as "already here" and IntelliJ then shows usages.
        if (targets.any { it.containingFile?.virtualFile == virtualFile && it.textRange?.contains(offset) == true }) {
            return null
        }
        return targets.toTypedArray()
    }

    private fun requestTargets(
        project: Project,
        virtualFile: VirtualFile,
        document: Document,
        offset: Int,
    ): List<PsiElement>? {
        val server = LanguageServerManager.getInstance(project).getLanguageServer(SERVER_ID)
        ProgressIndicatorUtils.awaitWithCheckCanceled(server)
        val item = server.getNow(null) ?: return null
        val features = item.clientFeatures
        val params = DefinitionParams(
            TextDocumentIdentifier(FileUriSupport.toString(virtualFile, features)),
            LSPIJUtils.toPosition(offset, document),
        )
        val request = item.server.textDocumentService.definition(params)
        try {
            ProgressIndicatorUtils.awaitWithCheckCanceled(request)
        } finally {
            if (!request.isDone) request.cancel(true)
        }
        val result = request.getNow(null) ?: return null
        return if (result.isLeft) {
            result.left.mapNotNull { LSPPsiElementFactory.toPsiElement(it, features, project) }
        } else {
            result.right.mapNotNull { LSPPsiElementFactory.toPsiElement(it, features, project) }
        }
    }

    private fun isCtrlHover(): Boolean =
        Thread.currentThread().stackTrace.any { it.className.startsWith(CTRL_MOUSE_HANDLER) }

    private companion object {
        const val SERVER_ID = "wowlua-ls"
        // The same check LSP4IJ's handler uses to tell Ctrl-hover from a click.
        const val CTRL_MOUSE_HANDLER = "com.intellij.codeInsight.navigation.CtrlMouseHandler2"
    }
}
