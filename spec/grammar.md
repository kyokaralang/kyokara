# Kyokara Formal Grammar

> PEG-style reference grammar for the Kyokara language (v0.0).
> This document is the authoritative specification for the parser.

## Notation

| Syntax       | Meaning                              |
|--------------|--------------------------------------|
| `'text'`     | Literal token                        |
| `RULE`       | Reference to another rule            |
| `a b`        | Sequence                             |
| `a / b`      | Ordered choice                       |
| `a?`         | Optional (zero or one)               |
| `a*`         | Zero or more                         |
| `a+`         | One or more                          |
| `(a)`        | Grouping                             |
| `!a`         | Not predicate (does not consume)     |
| `&a`         | And predicate (does not consume)     |

---

## Lexical Grammar

```peg
# ── Whitespace & Comments ────────────────────────────────────────────

Whitespace    <- [ \t\n\r]+
LineComment   <- '//' [^\n]*
BlockComment  <- '/*' BlockCommentBody '*/'
BlockCommentBody <- (BlockComment / !'*/' .)*

# ── Literals ─────────────────────────────────────────────────────────

IntLiteral    <- [0-9] [0-9_]*
FloatLiteral  <- [0-9] [0-9_]* '.' [0-9] [0-9_]*
StringLiteral <- '"' (StringChar)* '"'
StringChar    <- '\\' . / [^"\\]
CharLiteral   <- "'" ('\\' . / [^'\\]) "'"

# ── Identifiers & Keywords ───────────────────────────────────────────

Ident         <- [a-zA-Z_] [a-zA-Z0-9_]*  # not a keyword

Keyword       <- 'import' / 'as' / 'type' / 'fn' / 'let' / 'pub'
               / 'match' / 'cap' / 'with' / 'requires' / 'ensures'
               / 'invariant' / 'property' / 'for' / 'all' / 'where'
               / 'pipe' / 'old' / 'true' / 'false' / 'if' / 'else'
               / 'return'

# ── Operators & Delimiters ───────────────────────────────────────────

Arrow         <- '->'
FatArrow      <- '=>'
PipeGt        <- '|>'
EqEq          <- '=='
BangEq        <- '!='
GtEq          <- '>='
LtEq          <- '<='
# Single-char: = ! > < + - * / | & ? ( ) { } [ ] , : ; .
```

---

## Syntactic Grammar

### Source File

```peg
SourceFile     <- ImportDecl* Item* EOF
```

### Module & Imports

```peg
ImportDecl     <- 'import' Path ImportAlias?
ImportAlias    <- 'as' Ident
Path           <- Ident ('.' Ident)*
```

### Items

```peg
Item           <- 'pub'? (TypeDef
               /  FnDef
               /  CapDef
               /  PropertyDef
               /  LetBinding)
```

### Type Definitions

```peg
TypeDef        <- 'type' Ident TypeParamList? '=' TypeBody

TypeBody       <- VariantList / TypeExpr

VariantList    <- ('|' Variant)+
Variant        <- Ident VariantFieldList?
VariantFieldList <- '(' TypeExpr (',' TypeExpr)* ','? ')'

# Record types used inline
RecordFieldList <- '{' (RecordField (',' RecordField)* ','?)? '}'
RecordField    <- Ident ':' TypeExpr
```

### Function Definitions

```peg
FnDef          <- 'fn' Ident TypeParamList? ParamList ReturnType?
                  FnContract? BlockExpr

ParamList      <- '(' (Param (',' Param)* ','?)? ')'
Param          <- Ident ':' TypeExpr
ReturnType     <- '->' TypeExpr

FnContract     <- WithClause? PipeClause?
                  RequiresClause? EnsuresClause? InvariantClause?

WithClause     <- 'with' TypeExpr (',' TypeExpr)*
PipeClause     <- 'pipe' TypeExpr (',' TypeExpr)*
RequiresClause <- 'requires' '(' Expr ')'
EnsuresClause  <- 'ensures' '(' Expr ')'
InvariantClause <- 'invariant' '(' Expr ')'
```

### Capability Definitions

```peg
CapDef         <- 'cap' Ident TypeParamList? '{' CapMember* '}'
CapMember      <- FnDef
```

### Property Definitions

```peg
PropertyDef    <- 'property' Ident ParamList WhereClause? BlockExpr?
WhereClause    <- 'where' '(' Expr ')'
ForAllBinder   <- 'for' 'all' Ident ':' TypeExpr '.'
```

### Let Bindings

```peg
LetBinding     <- 'let' Pattern (':' TypeExpr)? '=' Expr
```

### Generics

```peg
TypeParamList  <- '<' TypeParam (',' TypeParam)* ','? '>'
TypeParam      <- Ident
TypeArgList    <- '<' TypeExpr (',' TypeExpr)* ','? '>'
```

### Type Expressions

```peg
TypeExpr       <- FnType
               /  RefinedType
               /  RecordType
               /  NameType

NameType       <- Path TypeArgList?
FnType         <- 'fn' '(' (TypeExpr (',' TypeExpr)*)? ')' '->' TypeExpr
RecordType     <- RecordFieldList
RefinedType    <- '{' Ident ':' TypeExpr '|' Expr '}'
```

### Expressions

Operator precedence (lowest to highest):

| Precedence | Operators      | Associativity |
|------------|----------------|---------------|
| 1          | `\|>`          | Left          |
| 2          | `==` `!=`      | Left          |
| 3          | `<` `>` `<=` `>=` | Left       |
| 4          | `+` `-`        | Left          |
| 5          | `*` `/`        | Left          |
| 6          | Unary `!` `-`  | Prefix        |
| 7          | `?` `.` `()`   | Postfix       |

```peg
Expr           <- PipelineExpr

PipelineExpr   <- EqualityExpr ('|>' EqualityExpr)*
EqualityExpr   <- ComparisonExpr (('==' / '!=') ComparisonExpr)*
ComparisonExpr <- AdditiveExpr (('<' / '>' / '<=' / '>=') AdditiveExpr)*
AdditiveExpr   <- MultiplicativeExpr (('+' / '-') MultiplicativeExpr)*
MultiplicativeExpr <- UnaryExpr (('*' / '/') UnaryExpr)*

UnaryExpr      <- ('!' / '-') UnaryExpr / PostfixExpr

PostfixExpr    <- PrimaryExpr (PostfixOp)*
PostfixOp      <- '?'                       # PropagateExpr
               /  '.' Ident                  # FieldExpr
               /  '(' ArgList ')'            # CallExpr

ArgList        <- (Arg (',' Arg)* ','?)?
Arg            <- NamedArg / Expr
NamedArg       <- Ident ':' Expr

PrimaryExpr    <- LiteralExpr
               /  IdentExpr
               /  PathExpr
               /  ParenExpr
               /  BlockExpr
               /  IfExpr
               /  MatchExpr
               /  RecordExpr
               /  ReturnExpr
               /  OldExpr
               /  LambdaExpr
               /  HoleExpr

LiteralExpr    <- IntLiteral / FloatLiteral / StringLiteral
               /  CharLiteral / 'true' / 'false'
IdentExpr      <- Ident
PathExpr       <- Path       # when Path has 2+ segments
ParenExpr      <- '(' Expr ')'
BlockExpr      <- '{' (LetBinding / Expr)* Expr? '}'
IfExpr         <- 'if' '(' Expr ')' BlockExpr ('else' (IfExpr / BlockExpr))?
MatchExpr      <- 'match' '(' Expr ')' MatchArmList
MatchArmList   <- '{' (MatchArm (',' MatchArm)* ','?)? '}'
MatchArm       <- Pattern '=>' Expr
RecordExpr     <- Path '{' RecordExprFieldList '}'
RecordExprFieldList <- (RecordExprField (',' RecordExprField)* ','?)?
RecordExprField <- Ident ':' Expr
ReturnExpr     <- 'return' Expr?
OldExpr        <- 'old' '(' Expr ')'
LambdaExpr     <- 'fn' '(' (Param (',' Param)*)? ')' '=>' Expr
HoleExpr       <- '_'
```

### Patterns

```peg
Pattern        <- ConstructorPat
               /  RecordPat
               /  LiteralPat
               /  WildcardPat
               /  IdentPat

IdentPat       <- Ident
ConstructorPat <- Path '(' PatList ')'
PatList        <- (Pattern (',' Pattern)* ','?)?
WildcardPat    <- '_'
LiteralPat     <- IntLiteral / FloatLiteral / StringLiteral
               /  CharLiteral / 'true' / 'false'
RecordPat      <- Path? '{' (Ident (',' Ident)* ','?)? '}'
```

---

## Reserved Words

All keywords listed above are reserved and cannot be used as identifiers.

```
all     as      cap     else    ensures   false    fn
for     if      import  invariant  let   match    old
pipe    property  pub   requires  return  true   type
where   with
```
