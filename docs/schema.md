# XML scalar mapping contract

## Record boundary

`XmlBatchBuilder::append_xml(&str)` accepts exactly one complete XML element.
The element's name does not determine output schema. Leading/trailing XML
whitespace, comments, processing instructions, and a leading XML declaration
are allowed. Multiple root elements, text outside the root, truncated input,
mismatched tags, and duplicate attributes fail.

Inputs are already-decoded Rust strings. `roxmltree` handles XML syntax,
character/name validation, references, and text normalization. Its default
parsing options reject DTDs; custom entities are not supported. XML declaration
version/encoding values are metadata and do not trigger decoding or select a
different conversion mode. This is not an XSD validator.

User namespace bindings and namespaced elements/attributes are rejected until
namespace URI matching is implemented in PR 2. They are never silently stripped.
An unused declaration of the predefined `xml` binding is harmless metadata.

## Paths

Paths are relative to the record root and contain unqualified XML names.

| Path | Selected value |
| --- | --- |
| `.` | Text of the record element itself |
| `@id` | Attribute on the record element |
| `title` | Text of a direct child |
| `author/name` | Text of a descendant |
| `author/@id` | Attribute on a descendant |

Paths are case sensitive. There is no XPath evaluation: absolute paths, empty
segments, wildcard selectors, predicates, namespace prefixes, and `..` are
configuration errors. `@name` is permitted only in the last segment. Column
names are nonempty and unique; multiple columns may map to the same XML path.

Selected text consists of concatenated direct text, entity references, and CDATA
in document order. Comments and processing instructions contribute no text.
A selected text element containing any child element is an error, including
otherwise empty children. Descendant paths can flatten nested containers without
selecting their parent's text. Unmapped content is ignored.

A selected scalar path must occur at most once. Repeated selected elements or
attribute paths fail even if the values are identical. Use future `List` mapping
for repeated data. No first-value or last-value policy is applied.

## Types and nulls

| Arrow type | Accepted present values |
| --- | --- |
| `Utf8` | Text with whitespace preserved after XML normalization |
| `Boolean` | `true`, `false`, `1`, `0` |
| `Int64` | Signed decimal integer accepted by Rust's `i64` parser, within range |
| `Float64` | Decimal floating-point value accepted by Rust's `f64` parser, finite only |

Numeric and boolean conversion trims XML whitespace (`space`, `tab`, `CR`, `LF`)
at the edges of the complete value. It does not trim individual text events.
Empty or ill-typed present values fail even when a field is nullable.

A missing nullable path produces null. A missing non-nullable path fails.
`<value/>`, `<value></value>`, and `value=""` are present empty strings; they
are not null. `xsi:nil` belongs to PR 2 and is not supported here.

The five predefined XML entities and decimal/hexadecimal character references
are decoded. Unknown entities and references to forbidden XML 1.0 characters
fail, including in unmapped content. CDATA stays literal. XML text/CDATA line
endings normalize CRLF and CR to LF; literal attribute whitespace is normalized
to spaces, while referenced whitespace retains the referenced character.

## Batches, projection, and errors

`XmlSchema::new` validates mappings before any records are accepted. Unsupported
Arrow types fail at configuration time. `batch_size` is a positive row count.
Full builders return `Error::BatchFull`; they do not flush implicitly.

`new_projected` selects schema indices in requested order. Duplicate or invalid
indices fail at construction. Missing-value, repetition, structure, and type
checks apply only to selected fields. XML parsing and supported-input checks
still apply to the entire record, including with an empty projection.
Zero-column batches retain the number of successfully appended records.

The entire selected row is parsed and converted before mutating any Arrow
column. Failed appends preserve all prior rows, consume no capacity, and leave
the builder usable. Field errors identify the output name and XML path. XML
syntax errors retain the parser's original error and line/column position.
No source URI or global record number is inferred by this in-memory API.

The cumulative byte count of each `Utf8` column is checked before appending.
Exceeding its signed 32-bit offset limit returns a field error without changing
the batch. Flushing can resolve cumulative overflow, but a single value larger
than that limit cannot be represented by `Utf8`.

`flush` returns the pending batch and resets the builder. An empty flush returns
`None`. Flush may emit fewer than `batch_size` rows.
