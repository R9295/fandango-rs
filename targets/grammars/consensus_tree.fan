<start> ::= '{"children": ' <children> '}\n' ;

<children> ::= '[]'
    | '[' <node> ']'
    | '[' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ', ' <node> ', ' <node> ']'
    | '[]'
    | '[]' ;
<node> ::= '{"children": ' <children> '}' ;
