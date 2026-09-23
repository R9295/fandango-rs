<start> ::= '{"actions": [' <actions> ']}\n' ;

<actions> ::= <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ', '
    <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ', '
    <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ', ' <slot_group> ;
<slot_group> ::= <action> ', ' <action> ', ' <action> | <action> ', ' <action> ;

<action> ::= '{"slot": ' <slot> ', "action": ' <kind> '}' ;
<slot> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10" | "11" | "12" | "13" | "14" | "15" ;
<kind> ::= '"NOTAR"' | '"FINALIZE"' | '"FAST_FINALIZE"' | '"REPLAY_COMPLETED"' | '"REPLAY_DEAD"' ;
