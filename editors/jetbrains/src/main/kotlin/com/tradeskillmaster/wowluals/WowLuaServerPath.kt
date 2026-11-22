package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.PluginPathManager
import com.intellij.util.EnvironmentUtil
import com.intellij.util.system.CpuArch
import com.intellij.util.system.OS
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.InvalidPathException
import java.nio.file.Path

/**
 * Locates the wowlua_ls binary: the user's own ([WowLuaServerPathSettings]), else
 * the plugin's bundled binary for this platform, else a `wowlua_ls` on PATH.
 * When there is none it can run, [Resolution.Problem] says why and what the user
 * can do; LSP4IJ shows it in its "Cannot start server" notification.
 */
object WowLuaServerPath {
    const val SETTINGS_PAGE = "Settings | Languages & Frameworks | WoW Lua LS"

    sealed interface Resolution {
        data class Found(val path: String) : Resolution
        data class Problem(val message: String) : Resolution
    }

    fun resolve(): Resolution = resolve(
        configured = WowLuaServerPathSettings.getInstance().serverPath,
        // <pluginPath>/server/, from this plugin's own dist directory.
        // PluginPathManager.getPluginResource is public API; it avoids the now-internal
        // PluginManagerCore.getPlugin() and a hardcoded plugin ID.
        serverDir = PluginPathManager.getPluginResource(javaClass, "server")?.toPath(),
        os = OS.CURRENT,
        arch = System.getProperty("os.arch"),
        // The PATH LSP4IJ starts the server with: the login shell's, which an IDE
        // launched from the Dock or a desktop entry doesn't have itself.
        pathDirs = (EnvironmentUtil.getValue("PATH") ?: System.getenv("PATH"))
            ?.split(File.pathSeparator).orEmpty(),
        home = System.getProperty("user.home"),
    )

    internal fun resolve(
        configured: String,
        serverDir: Path?,
        os: OS,
        arch: String,
        pathDirs: List<String>,
        home: String,
    ): Resolution {
        val binaryName = if (os == OS.Windows) "wowlua_ls.exe" else "wowlua_ls"
        val checkLoader = os != OS.Windows

        val own = configured.trim()
        if (own.isNotEmpty()) {
            val path = try {
                expandHome(own, home)
            } catch (_: InvalidPathException) {
                null
            }
            val problem = if (path == null) "is not a valid path" else unusableReason(path, checkLoader)
            return if (problem == null) Resolution.Found(path.toString())
            else Resolution.Problem("The server path set in $SETTINGS_PAGE, \"$own\", $problem.")
        }

        val platforms = bundledPlatforms(os, CpuArch.fromString(arch))
        var bundledLoader: String? = null
        if (serverDir != null) {
            for (platform in platforms) {
                val bundled = serverDir.resolve(platform).resolve(binaryName)
                if (!Files.isRegularFile(bundled)) continue
                // A bundled glibc build that can't load here (Alpine, NixOS) gives
                // way to one on PATH, which may well be built for this system.
                bundledLoader = if (checkLoader) missingLoader(bundled) else null
                if (bundledLoader == null) return Resolution.Found(bundled.toString())
                break
            }
        }

        for (dir in pathDirs) {
            if (dir.isBlank()) continue
            val candidate = File(dir, binaryName)
            if (candidate.isFile && candidate.canExecute()) return Resolution.Found(candidate.absolutePath)
        }

        val fix = "Build wowlua_ls on this machine and set its path in $SETTINGS_PAGE, or put it on your PATH."
        return Resolution.Problem(
            when {
                bundledLoader != null ->
                    "The bundled wowlua_ls server is built for glibc-based Linux and can't run here: it needs " +
                        "$bundledLoader, which this system doesn't have. $fix"
                // Without a server directory this is a development sandbox (`runIde`).
                serverDir == null -> "No wowlua_ls binary found on PATH. $fix"
                else -> "wowlua-ls doesn't include a server for $os $arch. $fix"
            },
        )
    }

    /**
     * The bundled `server/<platform>` directories that can run on [os]/[arch], best
     * first. Windows 11 on Arm runs x64 programs under emulation, so it falls back
     * to the x64 build until a native one ships.
     */
    internal fun bundledPlatforms(os: OS, arch: CpuArch): List<String> = when (os) {
        OS.Windows -> when (arch) {
            CpuArch.X86_64 -> listOf("win32-x64")
            CpuArch.ARM64 -> listOf("win32-arm64", "win32-x64")
            else -> emptyList()
        }
        OS.macOS -> when (arch) {
            CpuArch.ARM64 -> listOf("darwin-arm64")
            CpuArch.X86_64 -> listOf("darwin-x64")
            else -> emptyList()
        }
        OS.Linux -> when (arch) {
            CpuArch.X86_64 -> listOf("linux-x64")
            CpuArch.ARM64 -> listOf("linux-arm64")
            else -> emptyList()
        }
        else -> emptyList()
    }

    /** Why [path] can't run as the server, or null when nothing rules it out. */
    private fun unusableReason(path: Path, checkLoader: Boolean): String? {
        // The server starts in the project directory, so a relative path would be
        // checked here but run from there: a program the project chooses.
        if (!path.isAbsolute) return "is not an absolute path"
        if (!Files.exists(path)) return "doesn't exist"
        if (!Files.isRegularFile(path)) return "is not a file"
        if (checkLoader) {
            if (!Files.isExecutable(path)) return "is not executable (run chmod +x on it)"
            missingLoader(path)?.let { return "can't run on this system: it needs $it, which doesn't exist here" }
        }
        return null
    }

    private fun expandHome(path: String, home: String): Path = when {
        path == "~" -> Path.of(home)
        path.startsWith("~/") || path.startsWith("~\\") -> Path.of(home, path.substring(2))
        else -> Path.of(path)
    }

    /**
     * The dynamic loader a Linux binary names that this system lacks, or null.
     * Starting a binary whose loader is missing fails with "No such file or
     * directory", as if the binary itself were missing: a glibc build on Alpine,
     * or on NixOS.
     */
    private fun missingLoader(file: Path): String? {
        val loader = elfInterpreter(file) ?: return null
        return loader.takeIf { it.startsWith("/") && !Files.exists(Path.of(it)) }
    }

    private const val PT_INTERP = 3

    /**
     * The program interpreter (`PT_INTERP`) a 64-bit little-endian ELF file names,
     * or null for a file that isn't one or names none (a static binary).
     */
    internal fun elfInterpreter(file: Path): String? {
        try {
            FileChannel.open(file).use { channel ->
                fun read(position: Long, length: Int): ByteBuffer? {
                    val buf = ByteBuffer.allocate(length).order(ByteOrder.LITTLE_ENDIAN)
                    while (buf.hasRemaining()) {
                        if (channel.read(buf, position + buf.position()) <= 0) return null
                    }
                    return buf.flip()
                }

                val header = read(0, 64) ?: return null
                // "\u007fELF", then EI_CLASS 2 = 64-bit and EI_DATA 1 = little-endian:
                // every Linux target we build.
                if (header.getInt(0) != 0x464C457F || header.get(4).toInt() != 2 || header.get(5).toInt() != 1) {
                    return null
                }
                val phoff = header.getLong(32)
                val phentsize = header.getShort(54).toInt() and 0xFFFF
                val phnum = header.getShort(56).toInt() and 0xFFFF
                if (phentsize < 56) return null
                for (i in 0 until phnum) {
                    val ph = read(phoff + i.toLong() * phentsize, 56) ?: return null
                    if (ph.getInt(0) != PT_INTERP) continue
                    val size = ph.getLong(32)
                    if (size <= 0 || size > 4096) return null
                    val name = read(ph.getLong(8), size.toInt()) ?: return null
                    val bytes = name.array()
                    val end = bytes.indexOf(0.toByte()).takeIf { it >= 0 } ?: bytes.size
                    return String(bytes, 0, end, Charsets.ISO_8859_1)
                }
                return null
            }
        } catch (_: IOException) {
            return null
        } catch (_: IllegalArgumentException) {
            // A corrupt header's negative offset.
            return null
        }
    }
}
