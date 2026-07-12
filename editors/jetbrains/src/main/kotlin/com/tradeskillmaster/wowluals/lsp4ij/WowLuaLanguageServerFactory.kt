package com.tradeskillmaster.wowluals.lsp4ij

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.redhat.devtools.lsp4ij.LanguageServerEnablementSupport
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.features.LSPClientFeatures
import com.redhat.devtools.lsp4ij.client.features.LSPSemanticTokensFeature
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import com.tradeskillmaster.wowluals.WowLuaServerPath
import com.tradeskillmaster.wowluals.WowLuaSettings
import com.tradeskillmaster.wowluals.WowLuaStubDir

/**
 * The plugin's LSP client. LSP4IJ is a required dependency, so this is the sole
 * backend on every IDE — it serves files outside the project content (e.g.
 * go-to-definition targets inside WoW API stubs) and scopes servers strictly
 * per project.
 */
class WowLuaLanguageServerFactory : LanguageServerFactory, LanguageServerEnablementSupport {

    override fun createConnectionProvider(project: Project): StreamConnectionProvider {
        // Direct the server to materialize go-to-definition stub files into a
        // directory we watch and load into the VFS (see WowLuaStubDir): otherwise
        // LSP4IJ can't resolve the out-of-project target and go-to-definition on a
        // stub falls back to "find usages".
        WowLuaStubDir.ensureWatched()
        val env = mapOf(WowLuaStubDir.ENV_VAR to WowLuaStubDir.path().toString())
        return object : ProcessStreamConnectionProvider(
            listOf(WowLuaServerPath.resolve()),
            project.basePath ?: ".",
            env,
        ) {}
    }

    override fun createClientFeatures(): LSPClientFeatures =
        LSPClientFeatures().setSemanticTokensFeature(object : LSPSemanticTokensFeature() {
            // The server emits the non-standard `builtinConstant` token type for
            // boolean / nil literals inside `expression<>` strings; map it to the
            // IDE's constant color (IntelliJ only maps the standard LSP token types).
            override fun getTextAttributesKey(
                tokenType: String,
                tokenModifiers: List<String>,
                file: PsiFile,
            ): TextAttributesKey? =
                if (tokenType == "builtinConstant") DefaultLanguageHighlighterColors.CONSTANT
                else super.getTextAttributesKey(tokenType, tokenModifiers, file)
        })

    // LSP4IJ's "Language Servers" UI drives these, letting the user turn the
    // server off without uninstalling the plugin. Persisted in WowLuaSettings so
    // the choice survives restarts (the LSP4IJ base class only tracks it in memory).
    override fun isEnabled(project: Project): Boolean =
        !WowLuaSettings.getInstance().lsp4ijServerDisabled

    override fun setEnabled(enabled: Boolean, project: Project) {
        WowLuaSettings.getInstance().lsp4ijServerDisabled = !enabled
    }
}
