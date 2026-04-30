package com.tradeskillmaster.wowluals

import com.intellij.extapi.psi.PsiFileBase
import com.intellij.openapi.fileTypes.FileType
import com.intellij.psi.FileViewProvider

class WowLuaFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, WowLuaLanguage.INSTANCE) {
    override fun getFileType(): FileType = WowLuaFileType.INSTANCE
}
