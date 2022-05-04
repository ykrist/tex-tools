# doi-util re-write

- use CSL-JSON (possible relaxed) as the base input format; fuck BibTex, fuck BibLaTeX
- Schema is [here](https://github.com/citation-style-language/schema)
- JSON-schema for [Rust](https://docs.rs/jsonschema/latest/jsonschema/)
- `v_latexescape` for outputting BibTex/BibLatex

Getting CSL JSON from Crossref API:

```bash
curl -H "Accept: application/vnd.citationstyles.csl+json" https://api.crossref.org/works/10.1126/science.169.3946.635/transform
```

## CLI subcommands

- `fetch` : the "fill" command
- `clear` : clear the cache (subcaches?)
- `verify` : validate the input JSON

## Unicode to Latex

- normalize unicode strings first (want NFKC normalized form)
- map each character to an escape sequence
- examples: (stolen from `v_latexescape`)

```text
0x35 ->"\\#",
0x36 ->"\\$",
0x37 ->"\\%",
0x38 ->"\\&",
0x92 ->"\\textbackslash{}",
0x94 ->"\\textasciicircum{}",
0x95 ->"\\_",
0x123 ->"\\{",
0x125 ->"\\}",
0x126 ->"\\textasciitilde{}";
```

## CSL format

- Good documentation [here](https://docs.citationstyles.org/en/stable/specification.html#appendix-iv-variables)
- `issued` appears to be the "date" field.
- `publisher` for `report types will be used to store BibTex's institution field.
