package com.tradeskillmaster.wowluals

import com.intellij.lang.BracePair
import com.intellij.lang.PairedBraceMatcher
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType

class WowLuaBraceMatcher : PairedBraceMatcher {
    override fun getPairs(): Array<BracePair> = PAIRS

    override fun isPairedBracesAllowedBeforeType(lbraceType: IElementType, contextType: IElementType?): Boolean = true

    override fun getCodeConstructStart(file: PsiFile, openingBraceOffset: Int): Int = openingBraceOffset

    companion object {
        private val PAIRS = arrayOf(
            BracePair(WowLuaTokenTypes.LPAREN, WowLuaTokenTypes.RPAREN, false),
            BracePair(WowLuaTokenTypes.LBRACE, WowLuaTokenTypes.RBRACE, true),
            BracePair(WowLuaTokenTypes.LBRACKET, WowLuaTokenTypes.RBRACKET, false),
        )
    }
}
