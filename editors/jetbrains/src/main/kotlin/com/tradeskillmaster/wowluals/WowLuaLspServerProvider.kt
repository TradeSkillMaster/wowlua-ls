package com.tradeskillmaster.wowluals

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import java.io.File
import java.nio.file.Files

class WowLuaLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider {
        val commandLine = GeneralCommandLine(resolveServerPath())
        commandLine.workDirectory = File(project.basePath ?: ".")
        return OSProcessStreamConnectionProvider(commandLine)
    }

    private fun resolveServerPath(): String {
        val binaryName = if (System.getProperty("os.name").lowercase().contains("win"))
            "wowlua_ls.exe" else "wowlua_ls"

        val configured = WowLuaSettings.getInstance().serverPath
        if (configured.isNotBlank()) return configured

        val pluginPath = PluginManagerCore.getPlugin(
            PluginId.getId("com.tradeskillmaster.wowlua-ls")
        )?.pluginPath
        if (pluginPath != null) {
            val bundled = pluginPath.resolve("server").resolve(binaryName)
            if (Files.isExecutable(bundled)) return bundled.toString()
        }

        val pathDirs = System.getenv("PATH")?.split(File.pathSeparator).orEmpty()
        for (dir in pathDirs) {
            val candidate = File(dir, binaryName)
            if (candidate.isFile && candidate.canExecute()) return candidate.absolutePath
        }

        return binaryName
    }
}
