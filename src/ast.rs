#[derive(Clone, Debug, PartialEq)]
pub enum Modifier {
    Silent,
    Atomic,
    CompoundAtomic,
    NonAtomic,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InfixOp {
    Sequence,
    Choice,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrefixOp {
    PositivePredicate,
    NegativePredicate,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PostfixOp {
    Optional,
    Repeat,
    RepeatOnce,
    RepeatExact(u32),
    RepeatMin(u32),
    RepeatMax(u32),
    RepeatMinMax(u32, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminal {
    PushLiteral(String),
    Push(Box<Expression>),
    PeekSlice {
        start: Option<i64>,
        end: Option<i64>,
    },
    Identifier(String),
    String(String),
    InsensitiveString(String),
    Range {
        start: char,
        end: char,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Grouped(Box<Expression>),
    Terminal(Terminal),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub tag: Option<String>,
    pub prefix_ops: Vec<PrefixOp>,
    pub node: Node,
    pub postfix_ops: Vec<PostfixOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    pub leading_choice: bool,
    pub terms: Vec<Term>,
    pub infix_ops: Vec<InfixOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GrammarRule {
    pub name: String,
    pub modifier: Option<Modifier>,
    pub expression: Expression,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GrammarItem {
    Doc(String),
    Rule(GrammarRule),
    LineDoc(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grammar {
    pub items: Vec<GrammarItem>,
}
