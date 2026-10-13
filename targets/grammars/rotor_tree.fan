<start> ::= '{"children": ' <children> '}\n' ;

<children> ::= '[]'
    | '[]'
    | '[' <node> ']'
    | '[' <node> ']'
    | '[' <node> ']'
    | '[' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ', ' <node> ']' ;
<node> ::= '{"fecs": ' <fecs> ', "children": ' <children> '}' ;
<fecs> ::= '1' | '2' | '3' | '4' ;
