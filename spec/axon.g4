grammar Axon;

// ---------------------------------------------------------------------------
// AXON 2026 ANTLR4 Grammar
// ---------------------------------------------------------------------------

// A document is a sequence of top-level values
document: (discard* value)* discard* EOF;

discard: '#_' value;

// A value can be any of the core AXON types
value: node
     | literal
     | map
     | dict
     | list
     | temporal
     | binary
     | constant_ref
     | empty_set
     ;

// ---------------------------------------------------------------------------
// Node Styles
// ---------------------------------------------------------------------------

node: NAME                 # UnitNode
    | NAME brace_body      # BraceNode
    | NAME tuple_body      # TupleNode
    | NAME list_body       # ListNode
    ;

brace_body: '{' attribute* value* '}';
tuple_body: '(' value* ')';
list_body: '[' value* ']';

attribute: (NAME | STRING) ':' value;

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

map: '{' (value value)* '}';               // ordered map
dict: '{' (value ':' value)* '}';          // legacy python-style dict syntax support
list: '[' value* ']';

empty_set: '∅';

// ---------------------------------------------------------------------------
// Primitive Literals
// ---------------------------------------------------------------------------

literal: STRING
       | NUMBER
       | BOOL
       | NULL
       ;

temporal: CARET TEMPORAL_BODY;
binary: '|' BINARY_PAYLOAD '|';
constant_ref: '$' NAME;

// ---------------------------------------------------------------------------
// Lexer Rules
// ---------------------------------------------------------------------------

BOOL: 'true' | 'false';
NULL: 'null';
CARET: '^';

// Names follow UAX #31 identifier rules simplified here for ASCII/Latin
NAME: [a-zA-Z_] [a-zA-Z0-9_./-]*;

// Strings support double, single, and multiline backticks
STRING: '"' (~["\\] | '\\' .)* '"'
      | '\'' (~['\\] | '\\' .)* '\''
      | '`' (~[`\\] | '\\' .)* '`'
      ;

// Number matching the no-adjacency rule (maximal munch)
NUMBER: '-'? [0-9]+ ('_'? [0-9]+)* ('.' [0-9]+ ('_'? [0-9]+)*)? ([eE] [-+]? [0-9]+)? [dD]?;

TEMPORAL_BODY: [0-9]{4} '-' [0-9]{2} '-' [0-9]{2} ( [T ] [0-9]{2} ':' [0-9]{2} (':' [0-9]{2} ('.' [0-9]{1,9})?)? ('Z' | [-+][0-9]{2} (':'? [0-9]{2})?)?)?
             | [0-9]{2} ':' [0-9]{2} (':' [0-9]{2} ('.' [0-9]{1,9})?)? ('Z' | [-+][0-9]{2} (':'? [0-9]{2})?)?
             ;

// Base64 payload block (ignores whitespace)
BINARY_PAYLOAD: [A-Za-z0-9+/= \t\r\n]*;

// Whitespace and Comments
WS: [ \t\r\n]+ -> skip;
COMMENT: '#' ~[_] ~[\r\n]* -> skip;
