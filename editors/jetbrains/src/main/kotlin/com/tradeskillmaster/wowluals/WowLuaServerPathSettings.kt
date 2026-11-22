package com.tradeskillmaster.wowluals

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.RoamingType
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

/**
 * The user's own wowlua_ls binary (Settings | Languages & Frameworks | WoW Lua LS),
 * run instead of the bundled one.
 *
 * Application-level, never project-level: it names a program to run, so a
 * project's `.idea` files must not be able to set it. Kept apart from
 * [WowLuaSettings] so it doesn't roam: a path to a binary belongs to one
 * machine, and Backup and Sync carrying it to another would replace that
 * machine's bundled server with a path that isn't there.
 */
@State(
    name = "WowLuaLsServerPath",
    storages = [Storage(value = "wowlua-ls-server.xml", roamingType = RoamingType.DISABLED)],
)
class WowLuaServerPathSettings : PersistentStateComponent<WowLuaServerPathSettings.State> {
    class State {
        var serverPath: String = ""
    }

    private var state = State()

    /** Blank to run the bundled server. */
    var serverPath: String
        get() = state.serverPath
        set(value) { state.serverPath = value }

    override fun getState(): State = state

    override fun loadState(state: State) {
        this.state = state
    }

    companion object {
        fun getInstance(): WowLuaServerPathSettings =
            ApplicationManager.getApplication().getService(WowLuaServerPathSettings::class.java)
    }
}
