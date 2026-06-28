package com.tradeskillmaster.wowluals

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.application.PluginPathManager
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspSemanticTokensSupport
import java.io.File
import java.nio.file.Files

class WowLuaLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerSupportProvider.LspServerStarter) {
        if (file.extension == "lua" || file.extension == "toc") {
            serverStarter.ensureServerStarted(WowLuaLspServerDescriptor(project))
        }
    }
}

private class WowLuaLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "WoW Lua LS") {
    // The server emits the non-standard `builtinConstant` semantic-token type for
    // boolean / nil literals inside `expression<>` strings (so VS Code can map it to
    // `constant.language`). IntelliJ only maps the standard LSP token types, so map it
    // here to the IDE's constant color — otherwise `true`/`false`/`nil` would lose their
    // highlighting (fall back to the surrounding string scope).
    override val lspCustomization: LspCustomization = object : LspCustomization() {
        override val semanticTokensCustomizer = object : LspSemanticTokensSupport() {
            override fun getTextAttributesKey(tokenType: String, tokenModifiers: List<String>): TextAttributesKey? =
                if (tokenType == "builtinConstant") DefaultLanguageHighlighterColors.CONSTANT
                else super.getTextAttributesKey(tokenType, tokenModifiers)
        }
    }

    override fun isSupportedFile(file: VirtualFile) = file.extension == "lua" || file.extension == "toc"

    override fun createCommandLine(): GeneralCommandLine {
        val commandLine = GeneralCommandLine(resolveServerPath())
        commandLine.workDirectory = File(project.basePath ?: ".")
        return commandLine
    }

    private fun resolveServerPath(): String {
        val osName = System.getProperty("os.name").lowercase()
        val arch = System.getProperty("os.arch").lowercase()
        val isWindows = osName.contains("win")
        val binaryName = if (isWindows) "wowlua_ls.exe" else "wowlua_ls"

        val platform = when {
            isWindows -> "win32-x64"
            osName.contains("mac") -> if (arch == "aarch64") "darwin-arm64" else "darwin-x64"
            else -> "linux-x64"
        }

        val configured = WowLuaSettings.getInstance().serverPath
        if (configured.isNotBlank()) return configured

        // Resolve <pluginPath>/server/<platform>/ from this plugin's own dist directory.
        // PluginPathManager.getPluginResource is public API; it avoids the now-internal
        // PluginManagerCore.getPlugin() and a hardcoded plugin ID.
        val serverDir = PluginPathManager.getPluginResource(javaClass, "server")
        if (serverDir != null) {
            val bundled = serverDir.toPath().resolve(platform).resolve(binaryName)
            if (Files.isRegularFile(bundled)) return bundled.toString()
        }

        val pathDirs = System.getenv("PATH")?.split(File.pathSeparator).orEmpty()
        for (dir in pathDirs) {
            val candidate = File(dir, binaryName)
            if (candidate.isFile && candidate.canExecute()) return candidate.absolutePath
        }

        return binaryName
    }
}
