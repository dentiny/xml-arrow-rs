# XML mapping contract

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

Namespace resolution is handled by `roxmltree`; field paths match namespace URI
and local name. Unmapped namespaced content is allowed.

## Paths

Top-level paths are relative to the record root; nested paths are relative to
their selected parent element. Names may be unqualified or schema-prefixed.

| Path | Selected value |
| --- | --- |
| `.` | Text of the record element itself |
| `@id` | Attribute on the record element |
| `title` | Text of a direct child |
| `author/name` | Text of a descendant |
| `author/@id` | Attribute on a descendant |
| `b:author/b:name` | Namespaced descendants using a schema prefix |

Paths are case sensitive. There is no XPath evaluation: absolute paths, empty
segments, wildcard selectors, predicates, unbound namespace prefixes, and `..` are
configuration errors. `@name` is permitted only in the last segment. Column
names are nonempty and unique; multiple columns may map to the same XML path.

Selected text consists of concatenated direct text, entity references, and CDATA
in document order. Comments and processing instructions contribute no text.
A selected text element containing any child element is an error, including
otherwise empty children. Descendant paths can flatten nested containers without
selecting their parent's text. Unmapped content is ignored.

A selected scalar path must occur at most once. Repeated selected elements or
attribute paths fail even if the values are identical. Use a `List` item mapping
for repeated data. No first-value or last-value policy is applied.

## Nested fields

`XmlField::structure(name, path, nullable, children)` maps one selected element
to a `Struct`. Child paths are relative to that element. A missing or nil nullable
struct is null without evaluating its required children; Arrow child arrays are
aligned beneath the parent validity mask. A present struct still validates its
required children. Child column names are unique within each struct.

`XmlField::list(name, path, nullable, item)` selects one container element. The
item's path selects zero or more values inside it, in document order. The item
may itself be a scalar, struct, or list. Use `.` as the container path for
unwrapped repeated children of the current element. Container paths for structs
and lists must select elements, not attributes.

| XML state | List value |
| --- | --- |
| Existing container with no matching items | Empty list `[]` |
| Missing nullable container | Null list |
| Nil nullable container | Null list |
| Nil selected item with nullable item mapping | Null item |
| Missing/nil non-nullable container or nil non-nullable item | Error |

The container path must select at most one element; repetition is permitted only
at the list item path. Attribute item paths select existing attributes; absent
attributes contribute no item. Use a struct item with a nullable attribute child
when each repeated element must retain its own position.

Mapped struct/list elements containing both non-whitespace direct text and child
elements fail as mixed content. Scalar text still rejects child elements. A
struct with attributes and a `.` text child can represent a text-only element.
Other unmapped content is ignored.

## Namespaces

`XmlSchema::with_namespaces(fields, bindings)` accepts
`NamespaceBinding { prefix: "b", uri: "urn:books" }` entries.
The input can use another prefix or a default namespace for the same URI.
`xml` is predefined; `xml` and `xmlns` cannot be rebound. Duplicate prefixes,
empty URIs, and reserved URI bindings are configuration errors.

Unprefixed element and attribute paths match only names without a namespace.
The default XML namespace does not apply to attributes. `xmlns=""` resets an
element to no namespace. Prefix bindings are shared by nested mappings and
preserved during projection; source prefix spelling never changes matching.

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
are not null.

`xsi:nil` is recognized by the URI `http://www.w3.org/2001/XMLSchema-instance`,
regardless of its prefix. `true`/`1` produces null for a nullable mapped element;
`false`/`0` uses normal conversion. Other values fail. Nil elements may have
attributes, comments, and formatting whitespace, but no child elements or
non-whitespace text. Required mapped elements reject nil. These checks apply to
selected elements; this converter does not perform full XSD validation.

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

The cumulative byte count of each `Utf8` builder and item count of each `List`
builder is checked before appending, including beneath structs and nested lists.
All items in a row share their respective child builders, so checks include
their combined offsets. Exceeding a signed 32-bit offset limit returns a field
error without changing the batch. Flushing can resolve cumulative overflow,
but cannot represent an individual value exceeding that format limit.

`flush` returns the pending batch and resets the builder. An empty flush returns
`None`. Flush may emit fewer than `batch_size` rows.

## Inference

`XmlReader::infer` shares document validation, record selection, and XML limits
with the explicit reader. Sampling is bounded by record count and serialized
record bytes; sampled records and at most one lookahead record are replayed in
source order, with their original offsets. Projection applies after inference
and addresses the resulting field order. No matching records yields an empty
schema and no batches.

All inferred fields and list items are nullable. Missing fields and `xsi:nil`
therefore need no guess about requiredness. Nil-only fields default to `Utf8`.
A present empty scalar remains an empty string; when other samples establish a
structured field, an empty element can represent that empty structure.

Scalar inference trims XML whitespace to classify `true`/`false` as `Boolean`,
then tries `Int64`, finite `Float64`, and finally `Utf8`. `0` and `1` are integers.
Integer/float mixtures widen to `Float64`, with its normal precision limits;
other scalar type mixtures widen to `Utf8`. Empty scalar values force `Utf8`.
Text on an element with attributes uses a `.` child of type `Utf8`. Comments and
processing instructions do not contribute text.

Nested elements with attributes or child elements become structs. Repeated
sibling elements become lists using the existing unwrapped-list mapping.
Attribute paths and output names use `@name`; namespaced paths use prefixes.
Namespace identity follows URI, not input prefix spelling. Explicit bindings
supply prefixes for their URIs; when several aliases name one URI, the smallest
prefix is used. Other sampled URIs receive `ns0`, `ns1`, etc. in URI order,
avoiding assigned prefixes. Fields sort by URI (no namespace first), then name.

Scalar/struct conflicts and mixed text with child elements fail inference.
The resulting schema is fixed: later new fields and other unmapped content are
ignored. Values that violate inferred mappings, such as invalid numeric text or
repetition of a scalar, return conversion errors. The unfinished output batch is
discarded, as with explicit conversion errors.
