# Scarp में बहुत बड़े पूर्णांक: BigInt

यह **अभी अप्रकाशित development feature** है। Published `v0.0.6-alpha.5`
में यह सुविधा उपलब्ध नहीं है। इस feature
branch के source से compiler बनाना जरूरी है; सिर्फ version देखकर support
न मानें। [पूरी reference और limits](../bigint.md) देखें।

साधारण `integer` में signed 64-bit संख्या रहती है:
`-9223372036854775808` से `9223372036854775807` तक। `bigint` अलग type है,
जिससे इससे बहुत बड़ी पूर्णांक संख्याओं पर exact calculation कर सकते हैं।
यह decimal fractions रखने वाला type नहीं है।

```scarp
bind n: bigint = bigint("999999999999999999999");
print(n + bigint("1")); // 1000000000000000000000
```

बड़ी संख्या quotation marks के अंदर दें। `bigint(123)` गलत है क्योंकि
constructor को string चाहिए। मौजूदा integer को बदलने के लिए
`bigint_from_integer(123)` लिखें। Leading zeros स्वीकार हैं, `-000` का
result `0` है। Spaces, `_`, decimal point और हिन्दी अंक स्वीकार नहीं हैं;
constructor में ASCII `0`–`9` इस्तेमाल करें।

```scarp
var answer = bigint_from_integer(100);
answer *= bigint("100000000000000000000");
print(answer);
print(answer > bigint("100"));
```

दो BigInts पर `+`, `-`, `*`, `/`, `%` और comparisons काम करते हैं। Integer
या float के साथ अपने-आप conversion नहीं होता। Division zero की तरफ truncate
होता है: `-7 / 3` का quotient `-2`, remainder `-1` है। BigInts बनाकर भी यही
नियम रहता है। `bigint_abs(n)` absolute value देता है।

`bigint_to_integer(n)` तभी सफल होगा जब result signed 64-bit में आ सके;
बहुत बड़ी value को छोटा या round करके नहीं लौटाया जाता। Pure functions,
typed arguments/results और modules में runtime functions supported हैं।
Module constant initializer में `bigint(...)` call अभी supported नहीं है।

Default limit 10,000 digits है। जरूरत होने पर `--max-bigint-digits` बढ़ाकर
अधिकतम 1,000,000 कर सकते हैं। अलग byte quota और work budget भी लागू रहते हैं:

```sh
scarp exec --max-bigint-digits 100000 --max-steps 5000000 large.srp
scarp check --max-bigint-digits 100000 --max-steps 5000000 large.srp
```

`exec` output देता है; `check` वही selected calculations बिना output करता है।
`verify` केवल static checking करता है और malformed decimal string जैसी runtime
गलती को नहीं जांचता। बड़ी संख्या support होने से उसकी हर multiplication या
division तेज या असीमित नहीं हो जाती। Million-digit सामान्य multiplication/
division का performance अभी प्रमाणित नहीं है।

[Fibonacci और factorial का runnable example](../../examples/bigint_sequences.srp)
और [arithmetic example](../../examples/bigint_arithmetic.srp) exact expected
output के साथ दिए गए हैं। BigDecimal और BigInt bitwise operations future work हैं।
