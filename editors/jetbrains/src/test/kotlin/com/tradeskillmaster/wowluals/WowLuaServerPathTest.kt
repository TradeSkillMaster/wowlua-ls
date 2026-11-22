package com.tradeskillmaster.wowluals

import com.intellij.util.system.CpuArch
import com.intellij.util.system.OS
import com.tradeskillmaster.wowluals.WowLuaServerPath.Resolution
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeFalse
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.attribute.PosixFilePermissions

/** Which binary the plugin runs, and what it tells a user it can't run one for. */
class WowLuaServerPathTest {
    @get:Rule
    val temp = TemporaryFolder()

    private val root: Path get() = temp.root.toPath()

    private fun file(relative: String, contents: ByteArray = ByteArray(0), executable: Boolean = true): Path {
        val path = root.resolve(relative)
        Files.createDirectories(path.parent)
        Files.write(path, contents)
        if (!isWindows) {
            val mode = if (executable) "rwxr-xr-x" else "rw-r--r--"
            Files.setPosixFilePermissions(path, PosixFilePermissions.fromString(mode))
        }
        return path
    }

    private fun resolve(
        os: OS,
        arch: String,
        configured: String = "",
        serverDir: Path? = root.resolve("server"),
        pathDirs: List<String> = emptyList(),
    ): Resolution = WowLuaServerPath.resolve(configured, serverDir, os, arch, pathDirs, home = root.toString())

    private fun problem(resolution: Resolution): String =
        (resolution as? Resolution.Problem)?.message ?: throw AssertionError("expected a problem, got $resolution")

    @Test
    fun mapsEachPlatformToTheBuildItCanRun() {
        // The `os.arch` values JBR reports.
        val cases = listOf(
            Triple(OS.Linux, "amd64", listOf("linux-x64")),
            Triple(OS.Linux, "aarch64", listOf("linux-arm64")),
            Triple(OS.macOS, "aarch64", listOf("darwin-arm64")),
            Triple(OS.macOS, "x86_64", listOf("darwin-x64")),
            Triple(OS.Windows, "amd64", listOf("win32-x64")),
            // Windows 11 on Arm emulates x64; a native build would win once one ships.
            Triple(OS.Windows, "aarch64", listOf("win32-arm64", "win32-x64")),
            Triple(OS.Linux, "riscv64", emptyList()),
            Triple(OS.Linux, "arm", emptyList()),
            Triple(OS.Linux, "x86", emptyList()),
            Triple(OS.FreeBSD, "amd64", emptyList()),
            Triple(OS.Other, "amd64", emptyList()),
        )
        for ((os, arch, platforms) in cases) {
            assertEquals("$os $arch", platforms, WowLuaServerPath.bundledPlatforms(os, CpuArch.fromString(arch)))
        }
    }

    @Test
    fun runsTheBinaryBundledForThisPlatform() {
        file("server/linux-x64/wowlua_ls")
        val arm = file("server/linux-arm64/wowlua_ls")
        assertEquals(Resolution.Found(arm.toString()), resolve(OS.Linux, "aarch64"))
    }

    @Test
    fun windowsOnArmRunsTheX64BuildUnderEmulation() {
        val x64 = file("server/win32-x64/wowlua_ls.exe")
        assertEquals(Resolution.Found(x64.toString()), resolve(OS.Windows, "aarch64"))
    }

    @Test
    fun anUnsupportedPlatformFallsBackToPath() {
        file("server/linux-x64/wowlua_ls")
        val onPath = file("bin/wowlua_ls")
        assertEquals(
            Resolution.Found(onPath.toFile().absolutePath),
            resolve(OS.Linux, "riscv64", pathDirs = listOf("", root.resolve("empty").toString(), onPath.parent.toString())),
        )
    }

    @Test
    fun anUnsupportedPlatformWithNothingOnPathSaysWhatToDo() {
        file("server/linux-x64/wowlua_ls")
        val message = problem(resolve(OS.FreeBSD, "amd64"))
        assertEquals(
            "wowlua-ls doesn't include a server for FreeBSD amd64. Build wowlua_ls on this machine and set its " +
                "path in Settings | Languages & Frameworks | WoW Lua LS, or put it on your PATH.",
            message,
        )
    }

    @Test
    fun aBundledGlibcBuildWithoutItsLoaderIsReportedNotLaunched() {
        assumeFalse(isWindows)
        file("server/linux-x64/wowlua_ls", elf(MISSING_LOADER))
        val message = problem(resolve(OS.Linux, "amd64"))
        assertTrue(message, message.startsWith("The bundled wowlua_ls server is built for glibc-based Linux"))
        assertTrue(message, message.contains(MISSING_LOADER))
        assertTrue(message, message.contains(WowLuaServerPath.SETTINGS_PAGE))
    }

    @Test
    fun aBundledGlibcBuildWithoutItsLoaderGivesWayToPath() {
        assumeFalse(isWindows)
        file("server/linux-x64/wowlua_ls", elf(MISSING_LOADER))
        val onPath = file("bin/wowlua_ls")
        assertEquals(
            Resolution.Found(onPath.toFile().absolutePath),
            resolve(OS.Linux, "amd64", pathDirs = listOf(onPath.parent.toString())),
        )
    }

    @Test
    fun aBundledBinaryWhoseLoaderExistsOrThatNeedsNoneRuns() {
        assumeFalse(isWindows)
        val loader = file("lib/ld-test.so")
        val bundled = file("server/linux-x64/wowlua_ls", elf(loader.toString()))
        assertEquals(Resolution.Found(bundled.toString()), resolve(OS.Linux, "amd64"))
        file("server/linux-x64/wowlua_ls", elf(null))
        assertEquals(Resolution.Found(bundled.toString()), resolve(OS.Linux, "amd64"))
    }

    @Test
    fun theConfiguredPathWinsOverTheBundledBinary() {
        file("server/linux-x64/wowlua_ls")
        val own = file("own/wowlua_ls")
        assertEquals(Resolution.Found(own.toString()), resolve(OS.Linux, "amd64", configured = "  $own  "))
        assertEquals(Resolution.Found(own.toString()), resolve(OS.Linux, "amd64", configured = "~/own/wowlua_ls"))
    }

    @Test
    fun aConfiguredPathThatCantRunSaysWhyInsteadOfFallingBack() {
        file("server/linux-x64/wowlua_ls")
        val missing = root.resolve("nowhere/wowlua_ls")
        assertEquals(
            Resolution.Problem(
                "The server path set in Settings | Languages & Frameworks | WoW Lua LS, \"$missing\", doesn't exist.",
            ),
            resolve(OS.Linux, "amd64", configured = missing.toString()),
        )
        Files.createDirectories(root.resolve("own"))
        assertTrue(problem(resolve(OS.Linux, "amd64", configured = root.resolve("own").toString())).endsWith("is not a file."))
    }

    @Test
    fun aRelativeConfiguredPathIsRefusedSinceTheServerStartsInTheProject() {
        file("server/linux-x64/wowlua_ls")
        for (relative in listOf("wowlua_ls", ".cargo/bin/wowlua_ls", "./wowlua_ls")) {
            assertEquals(
                Resolution.Problem(
                    "The server path set in Settings | Languages & Frameworks | WoW Lua LS, \"$relative\", " +
                        "is not an absolute path.",
                ),
                resolve(OS.Linux, "amd64", configured = relative),
            )
        }
    }

    @Test
    fun aConfiguredPathThatIsntExecutableOrLacksItsLoaderSaysSo() {
        assumeFalse(isWindows)
        val download = file("Downloads/wowlua_ls", executable = false)
        assertTrue(
            problem(resolve(OS.Linux, "amd64", configured = download.toString()))
                .endsWith("is not executable (run chmod +x on it)."),
        )
        val glibcBuild = file("own/wowlua_ls", elf(MISSING_LOADER))
        val message = problem(resolve(OS.Linux, "amd64", configured = glibcBuild.toString()))
        assertTrue(message, message.endsWith("can't run on this system: it needs $MISSING_LOADER, which doesn't exist here."))
    }

    @Test
    fun aDevelopmentSandboxWithoutBundledBinariesUsesPath() {
        val onPath = file("bin/wowlua_ls")
        assertEquals(
            Resolution.Found(onPath.toFile().absolutePath),
            resolve(OS.Linux, "amd64", serverDir = null, pathDirs = listOf(onPath.parent.toString())),
        )
        assertTrue(problem(resolve(OS.Linux, "amd64", serverDir = null)).startsWith("No wowlua_ls binary found on PATH."))
    }

    @Test
    fun readsTheInterpreterAnElfFileNames() {
        assertEquals("/lib/ld-linux-aarch64.so.1", WowLuaServerPath.elfInterpreter(file("dynamic", elf("/lib/ld-linux-aarch64.so.1"))))
        assertNull(WowLuaServerPath.elfInterpreter(file("static", elf(null))))
        assertNull(WowLuaServerPath.elfInterpreter(file("script", "#!/bin/sh\nexit 0\n".toByteArray())))
        assertNull(WowLuaServerPath.elfInterpreter(file("empty")))
        assertNull(WowLuaServerPath.elfInterpreter(root.resolve("absent")))
        // Cut off inside the interpreter's program header.
        assertNull(WowLuaServerPath.elfInterpreter(file("truncated", elf("/lib/ld-musl-x86_64.so.1").copyOf(64 + 56 + 20))))
    }

    private companion object {
        const val MISSING_LOADER = "/nonexistent-wowlua-test/ld-linux-x86-64.so.2"
        val isWindows = OS.CURRENT == OS.Windows

        /**
         * A minimal 64-bit little-endian ELF file with a PT_LOAD program header and,
         * when [interp] is given, a PT_INTERP one naming it.
         */
        fun elf(interp: String?): ByteArray {
            val headers = if (interp == null) 1 else 2
            val interpOffset = 64 + 56 * headers
            val name = interp?.let { "$it\u0000".toByteArray(Charsets.ISO_8859_1) } ?: ByteArray(0)
            val buf = ByteBuffer.allocate(interpOffset + name.size).order(ByteOrder.LITTLE_ENDIAN)
            buf.put(0, 0x7F).put(1, 'E'.code.toByte()).put(2, 'L'.code.toByte()).put(3, 'F'.code.toByte())
            buf.put(4, 2) // ELFCLASS64
            buf.put(5, 1) // ELFDATA2LSB
            buf.put(6, 1) // EV_CURRENT
            buf.putLong(32, 64) // e_phoff
            buf.putShort(52, 64) // e_ehsize
            buf.putShort(54, 56) // e_phentsize
            buf.putShort(56, headers.toShort()) // e_phnum
            buf.putInt(64, 1) // PT_LOAD
            if (interp != null) {
                val ph = 64 + 56
                buf.putInt(ph, 3) // PT_INTERP
                buf.putLong(ph + 8, interpOffset.toLong()) // p_offset
                buf.putLong(ph + 32, name.size.toLong()) // p_filesz
                buf.put(interpOffset, name)
            }
            return buf.array()
        }
    }
}
