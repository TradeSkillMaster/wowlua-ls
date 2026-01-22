pub enum DiagnosticKind {
    NotClosedBlock,
    NotClosedComment,
    NotTerminatedString,

    UnexpectedKeyword,
    UnexpectedToken,
    UnexpectedOperator,

    ExpectingComma,
    ExpectingCommaOrBracket,
    ExpectingThen,
    ExpectingDo,
    ExpectingToken,
    ExpectingName,
    ExpectingClosingBracket,
    ExpectingFunctionCall,
    ExpectingExpression,

    InvalidName,
    InvalidFunction,
    InvalidNumberFormat,
}

pub struct Diagnostic {
    kind: DiagnosticKind,
    start: u32,
    end: u32,
}
