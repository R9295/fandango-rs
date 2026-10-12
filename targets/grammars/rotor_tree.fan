<start> ::= '{"blocks": [' <blocks> ']}\n' ;

<blocks> ::= <block>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks>
    | <block> ', ' <blocks> ;
<block> ::= '{"fecs": ' <fecs> '}' ;
<fecs> ::= '1' | '2' | '3' | '4' ;
