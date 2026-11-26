# String operators (development candidate)

The interpreter accepts typed string concatenation and repetition:

```scarp
bind name: string = "Sanskar";
bind language: string = "Scarp Programming Language";
print(name + "\n" + language);

var text = "hello";
text += "!";
text *= 2;
print(text);
```

- String + string returns a string. String * integer repeats; negative counts
  fail, zero produces empty text. The integer-left form remains a type error.
- No implicit conversion, no division-like newline operator, no string
  ordering, no mutation of shared string storage.
- Both operations reuse the existing bounded text kernels, even if user
  functions shadow the corresponding builtin names.
- Existing UTF-8 payload, work, per-value, and cumulative resource limits apply.
- Ordinary escapes retain their existing meanings. A raw literal r"a\nb"
  preserves the backslash and n. Triple-quoted literals preserve line breaks
  and other bytes verbatim, without escape decoding or indentation stripping.
- String interpolation is not introduced by this change. Experimental native
  backends continue to reject unsupported string expressions explicitly.
- This is a development candidate, not a claim of published release support.
