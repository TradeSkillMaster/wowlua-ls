package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.application.ApplicationManager
import com.redhat.devtools.lsp4ij.LSPFileSupport
import com.redhat.devtools.lsp4ij.LSPIJUtils
import com.redhat.devtools.lsp4ij.commands.LSPCommand
import com.redhat.devtools.lsp4ij.commands.LSPCommandAction
import com.redhat.devtools.lsp4ij.features.navigation.LSPDefinitionParams
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.TextDocumentIdentifier

/**
 * Handles the server's `wowlua-ls.showSuperDefinition` code-lens command (the
 * "overrides X" lens). LSP4IJ resolves a code-lens command by looking up an
 * IntelliJ action whose id equals the command string; there is no built-in
 * action for this one, so without this handler clicking the lens shows a
 * "missing command" error.
 *
 * The command carries `[uri, position]`. Mirroring the VS Code extension (which
 * forwards the same arguments to `editor.action.goToDefinition`), we send a
 * `textDocument/definition` request at that position and open the resolved
 * target. Doing a real definition lookup — rather than just revealing `position`,
 * which is the overriding method's own location — keeps the two editors in step
 * if the server's override resolution ever changes (e.g. override -> super).
 */
class WowLuaGoToDefinitionAction : LSPCommandAction() {
    override fun commandPerformed(command: LSPCommand, e: AnActionEvent) {
        val project = e.project ?: return
        val uri = command.getArgumentAt(0, String::class.java) ?: return
        val position = command.getArgumentAt(1, Position::class.java) ?: return

        val file = LSPIJUtils.findResourceFor(uri) ?: return
        val psiFile = LSPIJUtils.getPsiFile(file, project) ?: return
        val document = LSPIJUtils.getDocument(file) ?: return

        // The support infers the document from `psiFile`, so the identifier is left
        // empty (as in LSP4IJ's own go-to-declaration handler). getDefinitions is
        // non-blocking; it returns a future resolved off the EDT.
        val params = LSPDefinitionParams(TextDocumentIdentifier(), position, LSPIJUtils.toOffset(position, document))
        LSPFileSupport.getSupport(psiFile).definitionSupport.getDefinitions(params)
            .thenAccept { locations ->
                val target = locations?.firstOrNull() ?: return@thenAccept
                // Navigation must run on the EDT.
                ApplicationManager.getApplication().invokeLater {
                    LSPIJUtils.openInEditor(target.location(), target.languageServer().clientFeatures, project)
                }
            }
    }

    // Reading PSI / document state to build the request happens on the EDT.
    override fun getCommandPerformedThread(): ActionUpdateThread = ActionUpdateThread.EDT
}
