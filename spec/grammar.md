# Kyokara Formal Grammar

> PEG-style reference grammar for Kyokara (v0 parser contract).
> This document tracks implemented parser behavior.

## Notation

| Syntax | Meaning |
|---|---|
| `'text'` | Literal token |
| `RULE` | Reference to another rule |
| `a b` | Sequence |
| `a / b` | Ordered choice |
| `a?` | Optional |
| `a*` | Zero or more |
| `a+` | One or more |
| `(a)` | Grouping |

---

## Lexical Grammar

```peg
# Whitespace & comments
Whitespace       <- [ \t\n\r]+
LineComment      <- '//' [^\n]*
BlockComment     <- '/*' BlockCommentBody '*/'
BlockCommentBody <- (BlockComment / !'*/' .)*

# Literals
IntLiteral       <- [0-9] [0-9_]*
FloatLiteral     <- [0-9] [0-9_]* '.' [0-9] [0-9_]*
StringLiteral    <- '"' (StringChar)* '"'
StringChar       <- '\\' . / [^"\\]
CharLiteral      <- "'" ('\\' . / [^'\\]) "'"

# Identifiers (that are not keywords)
Ident            <- [a-zA-Z_] [a-zA-Z0-9_]*

# Keywords
# Note: `cap` is lexed as a reserved keyword for targeted diagnostics, but
# item parsing rejects it and requires `effect` declarations.
Keyword          <- 'module' / 'import' / 'as' / 'type' / 'fn' / 'let' / 'pub'
                  / 'match' / 'cap' / 'effect' / 'with' / 'pipe' / 'contract'
                  / 'requires' / 'ensures' / 'invariant'
                  / 'property' / 'for' / 'all' / 'where'
                  / 'old' / 'true' / 'false' / 'if' / 'else' / 'return'

# Operators
Arrow            <- '->'
LeftArrow        <- '<-'
FatArrow         <- '=>'
PipeGt           <- '|>'
EqEq             <- '=='
BangEq           <- '!='
GtEq             <- '>='
LtEq             <- '<='
AmpAmp           <- '&&'
PipePipe         <- '||'
LtLt             <- '<<'
GtGt             <- '>>'

# Single-char tokens
# = ! > < + - * / % | & ^ ~ ? ( ) { } [ ] , : ; .
```

---

## Syntactic Grammar

### Source File

```peg
SourceFile       <- ModuleDecl? ImportDecl* Item* EOF
```

### Module & Imports

```peg
ModuleDecl       <- 'module' Path
ImportDecl       <- 'import' Path ImportAlias?
ImportAlias      <- 'as' Ident
Path             <- Ident ('.' Ident)*
```

### Items

```peg
Item             <- 'pub'? (TypeDef
                   / FnDef
                   / EffectDef
                   / PropertyDef
                   / LetBinding)
```

### Type Definitions

```peg
TypeDef          <- 'type' Ident TypeParamList? '=' TypeBody
TypeBody         <- VariantList / TypeExpr

# Canonical ADT syntax: no leading `|` before the first variant.
VariantList      <- Variant ('|' Variant)*
Variant          <- Ident VariantFieldList?
VariantFieldList <- '(' TypeExpr (',' TypeExpr)* ','? ')'

RecordFieldList  <- '{' (RecordField (',' RecordField)* ','?)? '}'
RecordField      <- Ident ':' TypeExpr
```

### Function Definitions

```peg
# Also supports method form: fn TypeName.method(...)
FnDef            <- 'fn' (Ident ('.' Ident)?) TypeParamList? ParamList ReturnType?
                    FnContract? BlockExpr

ParamList        <- '(' (Param (',' Param)* ','?)? ')'
# `self` without a type annotation is accepted syntactically; semantic checks
# enforce where omitted type annotations are valid.
Param            <- Ident (':' TypeExpr)?
ReturnType       <- '->' TypeExpr

# Parsed in canonical order: with, pipe, then optional contract section.
# Duplicate clauses and out-of-order contract clauses are diagnosed.
FnContract       <- WithClause? PipeClause? ContractSection?
WithClause       <- 'with' TypeExpr (',' TypeExpr)*
PipeClause       <- 'pipe' TypeExpr (',' TypeExpr)*
ContractSection  <- 'contract' ContractClause+
ContractClause   <- RequiresClause / EnsuresClause / InvariantClause
RequiresClause   <- 'requires' '(' Expr ')'
EnsuresClause    <- 'ensures' '(' Expr ')'
InvariantClause  <- 'invariant' '(' Expr ')'
# Contract-clause order is strict: requires* ensures* invariant*
# Direct clauses outside `contract` are rejected.
```

### Effect Definitions

```peg
# Label-only declarations.
EffectDef        <- 'effect' Ident
```

### Property Definitions

```peg
# Body is currently optional in parser recovery mode.
PropertyDef       <- 'property' Ident PropertyParamList WhereClause? BlockExpr?
PropertyParamList <- '(' (PropertyParam (',' PropertyParam)* ','?)? ')'
PropertyParam     <- Ident ':' TypeExpr '<-' Expr
WhereClause       <- 'where' '(' Expr ')'
ForAllBinder      <- 'for' 'all' Ident ':' TypeExpr '.'
```

### Let Bindings

```peg
LetBinding       <- 'let' Pattern (':' TypeExpr)? '=' Expr
```

### Generics

```peg
TypeParamList    <- '<' TypeParam (',' TypeParam)* ','? '>'
TypeParam        <- Ident
TypeArgList      <- '<' TypeExpr (',' TypeExpr)* ','? '>'
```

### Type Expressions

```peg
TypeExpr         <- FnType
                 / RefinedType
                 / RecordType
                 / NameType

NameType         <- Path TypeArgList?
FnType           <- 'fn' '(' (TypeExpr (',' TypeExpr)*)? ')' '->' TypeExpr
RecordType       <- RecordFieldList
RefinedType      <- '{' Ident ':' TypeExpr '|' Expr '}'
```

### Expressions

Operator precedence (lowest to highest):

| Precedence | Operators | Associativity |
|---|---|---|
| 1 | `\|>` | Left |
| 2 | `\|\|` | Left |
| 3 | `&&` | Left |
| 4 | `==` `!=` | Left |
| 5 | `<` `>` `<=` `>=` | Left |
| 6 | `\|` | Left |
| 7 | `^` | Left |
| 8 | `&` | Left |
| 9 | `<<` `>>` | Left |
| 10 | `+` `-` | Left |
| 11 | `*` `/` `%` | Left |
| 12 | Unary `!` `-` `~` | Prefix |
| 13 | Postfix `?` `.` `()` `[]` | Postfix |

```peg
Expr               <- PipelineExpr

PipelineExpr       <- OrExpr ('|>' OrExpr)*
OrExpr             <- AndExpr ('||' AndExpr)*
AndExpr            <- EqualityExpr ('&&' EqualityExpr)*
EqualityExpr       <- ComparisonExpr (('==' / '!=') ComparisonExpr)*
ComparisonExpr     <- BitOrExpr (('<' / '>' / '<=' / '>=') BitOrExpr)*
BitOrExpr          <- BitXorExpr ('|' BitXorExpr)*
BitXorExpr         <- BitAndExpr ('^' BitAndExpr)*
BitAndExpr         <- ShiftExpr ('&' ShiftExpr)*
ShiftExpr          <- AdditiveExpr (('<<' / '>>') AdditiveExpr)*
AdditiveExpr       <- MultiplicativeExpr (('+' / '-') MultiplicativeExpr)*
MultiplicativeExpr <- UnaryExpr (('*' / '/' / '%') UnaryExpr)*

UnaryExpr          <- ('!' / '-' / '~') UnaryExpr / PostfixExpr

PostfixExpr        <- PrimaryExpr PostfixOp*
PostfixOp          <- '?'                    # PropagateExpr
                   / '.' Ident               # FieldExpr
                   / '(' ArgList ')'         # CallExpr
                   / '[' Expr ']'            # IndexExpr

ArgList            <- (Arg (',' Arg)* ','?)?
Arg                <- NamedArg / Expr
NamedArg           <- Ident ':' Expr

PrimaryExpr        <- LiteralExpr
                   / IdentExpr
                   / PathExpr
                   / ParenExpr
                   / BlockExpr
                   / IfExpr
                   / MatchExpr
                   / RecordExpr
                   / ReturnExpr
                   / OldExpr
                   / LambdaExpr
                   / HoleExpr

LiteralExpr        <- IntLiteral / FloatLiteral / StringLiteral
                   / CharLiteral / 'true' / 'false'
IdentExpr          <- Ident
PathExpr           <- Path
ParenExpr          <- '(' Expr ')'
BlockExpr          <- '{' (LetBinding / Expr)* Expr? '}'
IfExpr             <- 'if' '(' Expr ')' BlockExpr ('else' (IfExpr / BlockExpr))?
MatchExpr          <- 'match' '(' Expr ')' MatchArmList
# Match arms accept optional commas; leading `|` is rejected.
MatchArmList       <- '{' (MatchArm (',' MatchArm)* ','?)? '}'
MatchArm           <- Pattern '=>' Expr
RecordExpr         <- Path '{' RecordExprFieldList '}'
RecordExprFieldList <- (RecordExprField (',' RecordExprField)* ','?)?
RecordExprField    <- Ident ':' Expr
ReturnExpr         <- 'return' Expr?
OldExpr            <- 'old' '(' Expr ')'
LambdaExpr         <- 'fn' '(' (Param (',' Param)*)? ')' '=>' Expr
HoleExpr           <- '_'
```

### Patterns

```peg
Pattern            <- ConstructorPat
                   / RecordPat
                   / LiteralPat
                   / WildcardPat
                   / IdentPat

IdentPat           <- Path
ConstructorPat     <- Path '(' PatList ')'
PatList            <- (Pattern (',' Pattern)* ','?)?
WildcardPat        <- '_'
LiteralPat         <- IntLiteral / FloatLiteral / StringLiteral
                   / CharLiteral / 'true' / 'false'
RecordPat          <- Path? '{' (Ident (',' Ident)* ','?)? '}'
```

---

## Reserved Words

All keywords listed above are reserved and cannot be used as identifiers.

```
all      as       cap      contract  effect   else     ensures
false    fn       for      if        import   invariant let
match    module   old      pipe      property pub      requires
return   true     type     where     with
```
