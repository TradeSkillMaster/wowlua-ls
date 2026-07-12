package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.PathManager
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtil
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/**
 * The directory the language server materializes WoW API stub files into so the
 * editor can open them for go-to-definition, shared with the server process via
 * the [ENV_VAR] environment variable (WowLuaLanguageServerFactory's connection
 * provider sets it on the server process; the server honors it).
 *
 * Why this is needed on JetBrains: LSP4IJ resolves go-to-definition targets
 * through IntelliJ's VFS (`VirtualFileManager.findFileByUrl`, which does **not**
 * refresh). A stub file written under an ordinary temp directory — outside every
 * project content root and unknown to the VFS — is therefore not resolvable, and
 * "Go to Declaration or Usages" silently falls back to *Find Usages* (the user
 * sees a list of call sites instead of the definition).
 *
 * Placing the directory under the IDE system path (stable, out of any project)
 * and, in [ensureWatched], loading it into the VFS + registering it as a watch
 * root makes the materialized files present in the VFS — and kept fresh as the
 * server writes more — so navigation resolves a real file.
 */
object WowLuaStubDir {
    /** Environment variable the server reads to override its materialize dir. */
    const val ENV_VAR: String = "WOWLUA_LS_STUB_DIR"

    private val LOG = logger<WowLuaStubDir>()

    /** Stable, IDE-scoped directory (shared across all projects/servers). */
    fun path(): Path = Paths.get(PathManager.getSystemPath(), "wowlua-ls-stubs")

    @Volatile
    private var watched = false

    /**
     * Ensure the stub directory exists, is loaded into the VFS, and is watched so
     * files the server materializes are refreshed in. Idempotent and cheap after
     * the first call — safe to invoke every time a server connection is created.
     */
    @Synchronized
    fun ensureWatched() {
        if (watched) return
        val dir = path()
        try {
            Files.createDirectories(dir)
            val lfs = LocalFileSystem.getInstance()
            // Watch so stub files the server writes later trigger a VFS refresh;
            // this also schedules a refresh of the newly-watched root.
            lfs.addRootToWatch(dir.toString(), true)
            // Additionally pull any files already on disk (from a previous session —
            // the server skips rewriting unchanged files, so those produce no watch
            // event) into the VFS. Use a non-refreshing lookup + async refresh so
            // this is safe regardless of the calling thread's read-lock state.
            lfs.findFileByNioFile(dir)?.let { VfsUtil.markDirtyAndRefresh(true, true, true, it) }
            watched = true
        } catch (e: Exception) {
            LOG.warn("Failed to set up WoW Lua stub directory watch at $dir", e)
        }
    }
}
