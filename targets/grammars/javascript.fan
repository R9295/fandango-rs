<start> ::= <statements> ;
<statements> ::= <statement> "\n" <statements> | "" ;
<statement> ::= <declaration> ";" | <expression_statement> | <function> | <block> | <if_statement> | <loop> | <return_statement> | <break_statement> | <continue_statement> ;
<block> ::= "{\n" <statements> "}" ;
<declaration> ::= <declaration_kind> " " <identifier> <initializer> ;
<declaration_kind> ::= "let" | "const" ;
<initializer> ::= " = " <expression> | "" ;
<expression_statement> ::= "(" <expression> ");" ;
<function> ::= "function " <identifier> "(" <parameters> ") " <block> ;
<parameters> ::= <parameter_list> | "" ;
<parameter_list> ::= <identifier> | <identifier> ", " <parameter_list> ;
<if_statement> ::= "if (" <expression> ") " <block> <else_clause> ;
<else_clause> ::= " else " <block> | "" ;
<loop> ::= <while_loop> | <do_loop> | <for_loop> | <for_each_loop> ;
<while_loop> ::= "while (" <expression> ") " <block> ;
<do_loop> ::= "do " <block> " while (" <expression> ");" ;
<for_loop> ::= "for (" <for_initializer> "; " <optional_expression> "; " <optional_expression> ") " <block> ;
<for_initializer> ::= <declaration> | <optional_expression> ;
<optional_expression> ::= <expression> | "" ;
<for_each_loop> ::= "for (" <declaration_kind> " " <identifier> <iteration_operator> <expression> ") " <block> ;
<iteration_operator> ::= " of " | " in " ;
<return_statement> ::= "return" <return_value> ";" ;
<return_value> ::= " " <expression> | "" ;
<break_statement> ::= "break;" ;
<continue_statement> ::= "continue;" ;

<expression> ::= <identifier> | <literal> | <object> | <array> | <binary> | <unary> | <call> | <member> | <subscript> | <assignment> | <update> | <native_expression> ;
<object> ::= "({" <properties> "})" ;
<properties> ::= <property_list> | "" ;
<property_list> ::= <identifier> ": " <expression> | <identifier> ": " <expression> ", " <property_list> ;
<array> ::= "[" <arguments> "]" ;
<arguments> ::= <argument_list> | "" ;
<argument_list> ::= <expression> | <expression> ", " <argument_list> ;
<binary> ::= "(" <expression> " " <binary_operator> " " <expression> ")" ;
<binary_operator> ::= "+" | "-" | "*" | "/" | "%" | "===" | "!==" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "&" | "|" | "^" ;
<unary> ::= "(" <unary_operator> <expression> ")" ;
<unary_operator> ::= "!" | "~" | "-" | "+" | "typeof " | "void " ;
<call> ::= "(" <expression> ")(" <arguments> ")" ;
<member> ::= "(" <expression> ")." <identifier> ;
<subscript> ::= "(" <expression> ")[" <expression> "]" ;
<assignment> ::= "(" <assignment_target> " " <assignment_operator> " " <expression> ")" ;
<assignment_target> ::= <identifier> | <member> | <subscript> | "(" <expression> ")" ;
<assignment_operator> ::= "=" | "+=" | "-=" | "*=" ;
<update> ::= "(" <assignment_target> <update_operator> ")" ;
<update_operator> ::= "++" | "--" ;

<native_expression> ::= <boxed_expression> | <map_expression> | <set_expression> | <weak_map_expression> | <weak_set_expression> | <array_buffer_expression> | <data_view_expression> | <typed_array_expression> | <atomics_expression> | <bigint_expression> | <math_expression> | <symbol_expression> | <proxy_expression> | <reflect_expression> | <promise_expression> | <array_builtin_expression> | <string_builtin_expression> | <object_builtin_expression> ;

<boxed_expression> ::= "new Object()" | "new Array(" <arguments> ")" | "new String(" <expression> ")" | "new Number(" <expression> ")" | "new Boolean(" <expression> ")" ;

<map_base> ::= "new Map()" | "new Map([[" <expression> ", " <expression> "]])" ;
<map_expression> ::= <map_base> | "(" <map_base> ").set(" <expression> ", " <expression> ")" | "(" <map_base> ").get(" <expression> ")" | "(" <map_base> ").has(" <expression> ")" | "(" <map_base> ").delete(" <expression> ")" | "(" <map_base> ").clear()" | "(" <map_base> ").keys()" | "(" <map_base> ").values()" | "(" <map_base> ").entries()" ;

<set_base> ::= "new Set()" | "new Set([" <arguments> "])" ;
<set_expression> ::= <set_base> | "(" <set_base> ").add(" <expression> ")" | "(" <set_base> ").has(" <expression> ")" | "(" <set_base> ").delete(" <expression> ")" | "(" <set_base> ").clear()" | "(" <set_base> ").keys()" | "(" <set_base> ").values()" | "(" <set_base> ").entries()" ;

<weak_map_base> ::= "new WeakMap()" | "new WeakMap([[" <object> ", " <expression> "]])" ;
<weak_map_expression> ::= <weak_map_base> | "(" <weak_map_base> ").set(" <object> ", " <expression> ")" | "(" <weak_map_base> ").get(" <object> ")" | "(" <weak_map_base> ").has(" <object> ")" | "(" <weak_map_base> ").delete(" <object> ")" ;

<weak_set_base> ::= "new WeakSet()" | "new WeakSet([" <object> "])" ;
<weak_set_expression> ::= <weak_set_base> | "(" <weak_set_base> ").add(" <object> ")" | "(" <weak_set_base> ").has(" <object> ")" | "(" <weak_set_base> ").delete(" <object> ")" ;

<byte_length> ::= "8" | "16" | "32" | "64" ;
<small_index> ::= "0" | "1" | "2" | "3" ;
<array_buffer_base> ::= "new ArrayBuffer(" <byte_length> ")" ;
<array_buffer_expression> ::= <array_buffer_base> | "(" <array_buffer_base> ").slice(" <small_index> ")" | "(" <array_buffer_base> ").slice(" <small_index> ", " <byte_length> ")" ;

<data_view_base> ::= "new DataView(" <array_buffer_base> ")" ;
<data_view_expression> ::= <data_view_base> | "(" <data_view_base> ").getInt8(" <small_index> ")" | "(" <data_view_base> ").getUint16(" <small_index> ", true)" | "(" <data_view_base> ").getFloat64(0, true)" | "(" <data_view_base> ").setInt8(" <small_index> ", " <number> ")" | "(" <data_view_base> ").setUint16(" <small_index> ", " <number> ", true)" | "(" <data_view_base> ").setFloat64(0, " <number> ", true)" ;

<typed_array_constructor> ::= "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array" | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array" ;
<typed_array_base> ::= "new " <typed_array_constructor> "(" <byte_length> ")" | "new " <typed_array_constructor> "([" <arguments> "])" | "new " <typed_array_constructor> "(" <array_buffer_base> ")" | "new BigInt64Array([" <bigint_literal> "])" | "new BigUint64Array([" <bigint_literal> "])" ;
<typed_array_expression> ::= <typed_array_base> | "(" <typed_array_base> ").at(" <small_index> ")" | "(" <typed_array_base> ").fill(" <number> ")" | "(" <typed_array_base> ").includes(" <expression> ")" | "(" <typed_array_base> ").indexOf(" <expression> ")" | "(" <typed_array_base> ").reverse()" | "(" <typed_array_base> ").slice(" <small_index> ")" | "(" <typed_array_base> ").sort()" | "(" <typed_array_base> ").subarray(" <small_index> ")" ;

<shared_integer_array> ::= "new Int32Array(new SharedArrayBuffer(" <byte_length> "))" | "new Uint32Array(new SharedArrayBuffer(" <byte_length> "))" ;
<atomics_expression> ::= "Atomics.add(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.sub(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.and(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.or(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.xor(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.exchange(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.compareExchange(" <shared_integer_array> ", 0, " <number> ", " <number> ")" | "Atomics.load(" <shared_integer_array> ", 0)" | "Atomics.store(" <shared_integer_array> ", 0, " <number> ")" | "Atomics.isLockFree(" <byte_length> ")" | "Atomics.wait(new Int32Array(new SharedArrayBuffer(" <byte_length> ")), 0, 0, 0)" | "Atomics.notify(new Int32Array(new SharedArrayBuffer(" <byte_length> ")), 0, 1)" ;

<bigint_literal> ::= "1n" | "2n" | "3n" | "4n" | "5n" | "6n" | "7n" | "8n" ;
<bigint_operator> ::= "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<<" | ">>" | "**" ;
<bigint_expression> ::= <bigint_literal> | "BigInt(" <number> ")" | "BigInt.asIntN(" <byte_length> ", " <bigint_literal> ")" | "BigInt.asUintN(" <byte_length> ", " <bigint_literal> ")" | "(" <bigint_literal> " " <bigint_operator> " " <bigint_literal> ")" ;

<math_unary> ::= "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "cbrt" | "ceil" | "clz32" | "cos" | "cosh" | "exp" | "expm1" | "floor" | "fround" | "log" | "log1p" | "log2" | "log10" | "round" | "sign" | "sin" | "sinh" | "sqrt" | "tan" | "tanh" | "trunc" ;
<math_binary> ::= "atan2" | "hypot" | "imul" | "max" | "min" | "pow" ;
<math_expression> ::= "Math." <math_unary> "(" <expression> ")" | "Math." <math_binary> "(" <expression> ", " <expression> ")" | "Math.random()" ;

<well_known_symbol> ::= "asyncIterator" | "hasInstance" | "isConcatSpreadable" | "iterator" | "match" | "matchAll" | "replace" | "search" | "species" | "split" | "toPrimitive" | "toStringTag" | "unscopables" ;
<symbol_base> ::= "Symbol()" | "Symbol(" <string> ")" | "Symbol.for(" <string> ")" ;
<symbol_expression> ::= <symbol_base> | "Symbol.keyFor(Symbol.for(" <string> "))" | "Symbol." <well_known_symbol> | "(" <symbol_base> ").toString()" | "(" <symbol_base> ").valueOf()" ;

<proxy_expression> ::= "new Proxy(" <object> ", " <object> ")" | "Proxy.revocable(" <object> ", " <object> ")" ;
<reflect_expression> ::= "Reflect.get(" <object> ", " <string> ")" | "Reflect.set(" <object> ", " <string> ", " <expression> ")" | "Reflect.has(" <object> ", " <string> ")" | "Reflect.ownKeys(" <object> ")" | "Reflect.deleteProperty(" <object> ", " <string> ")" | "Reflect.getPrototypeOf(" <object> ")" | "Reflect.setPrototypeOf(" <object> ", " <object> ")" | "Reflect.isExtensible(" <object> ")" | "Reflect.preventExtensions(" <object> ")" | "Reflect.defineProperty(" <object> ", " <string> ", {value: " <expression> "})" | "Reflect.apply(String, null, [" <expression> "])" | "Reflect.construct(Object, [" <expression> "])" ;

<promise_base> ::= "Promise.resolve(" <expression> ")" | "Promise.reject(" <expression> ")" | "new Promise((resolve, reject) => resolve(" <expression> "))" ;
<promise_expression> ::= <promise_base> | "(" <promise_base> ").then((value) => value)" | "(" <promise_base> ").catch((error) => error)" | "(" <promise_base> ").finally(() => 0)" | "Promise.all([" <promise_base> "])" | "Promise.allSettled([" <promise_base> "])" | "Promise.any([" <promise_base> "])" | "Promise.race([" <promise_base> "])" ;

<array_builtin_expression> ::= "Array.isArray(" <expression> ")" | "Array.from(" <array> ")" | "Array.of(" <arguments> ")" | "(" <array> ").at(" <small_index> ")" | "(" <array> ").concat(" <array> ")" | "(" <array> ").copyWithin(" <small_index> ", " <small_index> ")" | "(" <array> ").entries()" | "(" <array> ").fill(" <expression> ")" | "(" <array> ").filter((value) => value)" | "(" <array> ").find((value) => value)" | "(" <array> ").flat()" | "(" <array> ").includes(" <expression> ")" | "(" <array> ").indexOf(" <expression> ")" | "(" <array> ").join(" <string> ")" | "(" <array> ").keys()" | "(" <array> ").map((value) => value)" | "(" <array> ").pop()" | "(" <array> ").push(" <arguments> ")" | "(" <array> ").reduce((left, right) => left, " <expression> ")" | "(" <array> ").reverse()" | "(" <array> ").shift()" | "(" <array> ").slice(" <small_index> ")" | "(" <array> ").sort()" | "(" <array> ").splice(" <small_index> ", " <small_index> ")" | "(" <array> ").unshift(" <arguments> ")" | "(" <array> ").values()" ;

<string_builtin_expression> ::= "String(" <expression> ")" | "String.fromCharCode(" <number> ")" | "(" <string> ").at(" <small_index> ")" | "(" <string> ").charAt(" <small_index> ")" | "(" <string> ").charCodeAt(" <small_index> ")" | "(" <string> ").concat(" <string> ")" | "(" <string> ").endsWith(" <string> ")" | "(" <string> ").includes(" <string> ")" | "(" <string> ").indexOf(" <string> ")" | "(" <string> ").padEnd(" <byte_length> ", " <string> ")" | "(" <string> ").padStart(" <byte_length> ", " <string> ")" | "(" <string> ").repeat(" <small_index> ")" | "(" <string> ").replace(" <string> ", " <string> ")" | "(" <string> ").slice(" <small_index> ")" | "(" <string> ").split(" <string> ")" | "(" <string> ").startsWith(" <string> ")" | "(" <string> ").substring(" <small_index> ")" | "(" <string> ").toLowerCase()" | "(" <string> ").toUpperCase()" | "(" <string> ").trim()" ;

<object_builtin_expression> ::= "Object(" <expression> ")" | "Object.assign(" <object> ", " <object> ")" | "Object.create(" <object> ")" | "Object.defineProperty(" <object> ", " <string> ", {value: " <expression> "})" | "Object.entries(" <object> ")" | "Object.freeze(" <object> ")" | "Object.fromEntries([[" <string> ", " <expression> "]])" | "Object.getOwnPropertyNames(" <object> ")" | "Object.getPrototypeOf(" <object> ")" | "Object.hasOwn(" <object> ", " <string> ")" | "Object.is(" <expression> ", " <expression> ")" | "Object.isExtensible(" <object> ")" | "Object.isFrozen(" <object> ")" | "Object.isSealed(" <object> ")" | "Object.keys(" <object> ")" | "Object.preventExtensions(" <object> ")" | "Object.seal(" <object> ")" | "Object.setPrototypeOf(" <object> ", " <object> ")" | "Object.values(" <object> ")" ;

<literal> ::= <number> | <string> | "true" | "false" | "null" ;
<number> ::= <digit> | <nonzero> <digits> | <digit> "." <digits> ;
<digits> ::= <digit> | <digit> <digits> ;
<nonzero> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
<digit> ::= "0" | <nonzero> ;
<string> ::= '"' <string_characters> '"' ;
<string_characters> ::= <letter> <string_characters> | "" ;
<identifier> ::= "_" <identifier_tail> ;
<identifier_tail> ::= <letter> | <letter> <identifier_tail> ;
<letter> ::= "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" ;
