<start> ::= <scenario> ;

<scenario> ::= "alpenglow {\n"
    "  slot: s0\n"
    "  blocks: [b1, b2]\n"
    "  validators: 20\n"
    "  stake-per-validator: 5%\n"
    "  byzantine-stake: 20%\n"
    "  potentially-absent-stake: 20%\n"
    <byzantine_validators>
    <potentially_absent_validators>
    <honest_validators>
    "}\n" ;

<byzantine_validators> ::= <byzantine_v0> <byzantine_v1> <byzantine_v2> <byzantine_v3> ;
<byzantine_v0> ::= "  v0 { stake: 5%, role: byzantine, votes: [" <vote_list> "] }\n" ;
<byzantine_v1> ::= "  v1 { stake: 5%, role: byzantine, votes: [" <vote_list> "] }\n" ;
<byzantine_v2> ::= "  v2 { stake: 5%, role: byzantine, votes: [" <vote_list> "] }\n" ;
<byzantine_v3> ::= "  v3 { stake: 5%, role: byzantine, votes: [" <vote_list> "] }\n" ;

<potentially_absent_validators> ::= <potentially_absent_v4> <potentially_absent_v5> <potentially_absent_v6> <potentially_absent_v7> ;
<potentially_absent_v4> ::= "  v4 { stake: 5%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;
<potentially_absent_v5> ::= "  v5 { stake: 5%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;
<potentially_absent_v6> ::= "  v6 { stake: 5%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;
<potentially_absent_v7> ::= "  v7 { stake: 5%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;

<honest_validators> ::= <honest_v8> <honest_v9> <honest_v10> <honest_v11> <honest_v12> <honest_v13> <honest_v14> <honest_v15> <honest_v16> <honest_v17> <honest_v18> <honest_v19> ;
<honest_v8> ::= "  v8 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v9> ::= "  v9 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v10> ::= "  v10 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v11> ::= "  v11 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v12> ::= "  v12 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v13> ::= "  v13 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v14> ::= "  v14 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v15> ::= "  v15 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v16> ::= "  v16 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v17> ::= "  v17 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v18> ::= "  v18 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v19> ::= "  v19 { stake: 5%, role: honest, votes: [" <vote_list> "] }\n" ;

<optional_vote_list> ::= "" | <vote_list> ;
<vote_list> ::= <vote> | <vote> ", " <vote_list> ;

<vote> ::= <skip> | <notarize> | <notarize_fallback> | <skip_fallback> ;
<skip> ::= "skip" ;
<notarize> ::= "notarize(" <block> ")" ;
<notarize_fallback> ::= "notarize-fallback(" <block> ")" ;
<skip_fallback> ::= "skip-fallback" ;
<block> ::= "b1" | "b2" ;
