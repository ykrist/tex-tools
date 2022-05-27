# CLI Tools for Tex-related things

Currently consists of a single tool, `bib-db` which uses takes a [Citation Style Language (CSL) JSON](https://github.com/citation-style-language/schema) file of DOIs and cite-keys (entry IDs) and grabs bibliographic data from https://doi.org.  It can output either a BibLatex (**not** Bibtex) Database `.bib` (its main purpose) or CSL JSON.  Any field, like `author`, `type`, or `institution` can be manually overridden by specifying it in input file.  Non-ASCII UTF8 in fields is converted to appropriate TeX macros.
