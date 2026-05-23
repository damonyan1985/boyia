use super::span::Span;

#[derive(Clone, Debug)]
pub struct Program {
    pub items: Vec<TopLevelItem>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TopLevelItem {
    VarDecl(VarDecl),
    FunDecl(FunDecl),
    ClassDecl(ClassDecl),
    ExprStmt(ExprStmt),
}

#[derive(Clone, Debug)]
pub struct VarDecl {
    pub names: Vec<NameBinding>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct NameBinding {
    pub name: String,
    pub name_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunKind {
    Function,
    AsyncFunction,
    Method,
    AsyncMethod,
    PropMethod,
    AsyncPropMethod,
}

#[derive(Clone, Debug)]
pub struct FunDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<String>,
    pub body: Block,
    pub kind: FunKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub name: String,
    pub name_span: Span,
    pub extends: Option<(String, Span)>,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ClassMember {
    PropAssign { name: String, name_span: Span, span: Span },
    Method(FunDecl),
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    VarDecl(VarDecl),
    FunDecl(FunDecl),
    Return { span: Span },
    If { span: Span },
    While { span: Span },
    DoWhile { span: Span },
    For { span: Span },
    Break { span: Span },
    Await { span: Span },
    PropAssign { name: String, name_span: Span, span: Span },
    ExprStmt(ExprStmt),
    Block(Block),
}

#[derive(Clone, Debug)]
pub struct ExprStmt {
    pub requires: Vec<RequireCall>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct RequireCall {
    pub path: String,
    pub span: Span,
}
