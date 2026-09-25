<start> ::= '{"id": "0", "children": ' <children> '}\n' ;

<children> ::= '[]'
    | '[' <node> ']'
    | '[' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ', ' <node> ']'
    | '[' <node> ', ' <node> ', ' <node> ', ' <node> ']'
    | '[]'
    | '[]' ;
<node> ::= '{"id": "' <label> '", "children": ' <children> '}' ;

<label> ::= <depth> <letters> ;
<depth> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" ;
<letters> ::= <letter> | <letter> <letter> | <letter> <letter> <letter> | <letter> <letter> <letter> <letter> ;
<letter> ::= "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" ;
