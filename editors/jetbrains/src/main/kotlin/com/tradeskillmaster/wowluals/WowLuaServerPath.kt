package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.PluginPathManager
import java.io.File
import java.nio.file.Files

/**
 * Locates the wowlua_ls binary. Shared by both LSP backends (the built-in
 * client and LSP4IJ): settings override, then the plugin's bundled
 * per-platform binary, then PATH.
 */
object WowLuaServerPath {
    fun resolve(): String {
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
