package com.tradeskillmaster.wowluals

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import java.io.File
import java.nio.file.Files

class WowLuaLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerSupportProvider.LspServerStarter) {
        if (file.extension == "lua") {
            serverStarter.ensureServerStarted(WowLuaLspServerDescriptor(project))
        }
    }
}

private class WowLuaLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "WoW Lua LS") {
    override fun isSupportedFile(file: VirtualFile) = file.extension == "lua"

    override fun createCommandLine(): GeneralCommandLine {
        val commandLine = GeneralCommandLine(resolveServerPath())
        commandLine.workDirectory = File(project.basePath ?: ".")
        return commandLine
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
