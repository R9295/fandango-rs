<start> ::= '{"blocks": [' <blocks> ']}\n' ;

<blocks> ::= <block>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks> ;
<block> ::= '{"forks": ' <forks> '}' ;
<forks> ::= '[]'
    | '[]'
    | '[]'
    | '[' <fork> ']'
    | '[' <fork> ']'
    | '[' <fork> ', ' <fork> ']'
    | '[' <fork> ', ' <fork> ', ' <fork> ']'
    | '[' <fork> ', ' <fork> ', ' <fork> ', ' <fork> ']' ;
<fork> ::= '{}' ;
