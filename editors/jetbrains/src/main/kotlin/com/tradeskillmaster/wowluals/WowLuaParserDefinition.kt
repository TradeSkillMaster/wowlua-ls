package com.tradeskillmaster.wowluals

import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import com.intellij.extapi.psi.ASTWrapperPsiElement

class WowLuaParserDefinition : ParserDefinition {
    companion object {
        val FILE = IFileElementType(WowLuaLanguage.INSTANCE)
    }

    override fun createLexer(project: Project?): Lexer = WowLuaLexer()
    override fun createParser(project: Project?): PsiParser = WowLuaParser()
    override fun getFileNodeType(): IFileElementType = FILE
    override fun getCommentTokens(): TokenSet = WowLuaTokenTypes.COMMENTS
    override fun getStringLiteralElements(): TokenSet = WowLuaTokenTypes.STRINGS
    override fun createElement(node: ASTNode): PsiElement = ASTWrapperPsiElement(node)
    override fun createFile(viewProvider: FileViewProvider): PsiFile = WowLuaFile(viewProvider)
}
