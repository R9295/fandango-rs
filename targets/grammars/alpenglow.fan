<start> ::= '{"slots": [' <depths> ']}\n' ;

<depths> ::= <depth>
    | <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth>
    | <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ', ' <depth> ;
<depth> ::= <certified_depth> | <skipped_depth> ;

<certified_depth> ::= '{"slot": ' <slot> ', "outcome": ' <certified> ', "block": ' <block_id> ', "blocks": [' <blocks> ']}' ;
<skipped_depth> ::= '{"slot": ' <slot> ', "outcome": ' <skipped> ', "blocks": []}'
    | '{"slot": ' <slot> ', "outcome": ' <skipped> ', "blocks": [' <blocks> ']}' ;
<certified> ::= '"FAST_FINALIZE"' | '"FINALIZE"' | '"NOTAR_FALLBACK"' ;
<skipped> ::= '"SKIP"' | '"SKIP_FALLBACK"' ;

<blocks> ::= <block>
    | <block> ', ' <block>
    | <block> ', ' <block> ', ' <block>
    | <block> ', ' <block> ', ' <block> ', ' <block>
    | <block> ', ' <block> ', ' <block> ', ' <block> ', ' <block> ;
<block> ::= '{"id": ' <block_id> ', "parent": ' <block_id> '}' ;

<slot> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" ;
<block_id> ::= '"0"'
    | '"1a"' | '"1b"' | '"1c"' | '"1d"' | '"1e"'
    | '"2a"' | '"2b"' | '"2c"' | '"2d"' | '"2e"'
    | '"3a"' | '"3b"' | '"3c"' | '"3d"' | '"3e"'
    | '"4a"' | '"4b"' | '"4c"' | '"4d"' | '"4e"'
    | '"5a"' | '"5b"' | '"5c"' | '"5d"' | '"5e"'
    | '"6a"' | '"6b"' | '"6c"' | '"6d"' | '"6e"'
    | '"7a"' | '"7b"' | '"7c"' | '"7d"' | '"7e"'
    | '"8a"' | '"8b"' | '"8c"' | '"8d"' | '"8e"' ;
